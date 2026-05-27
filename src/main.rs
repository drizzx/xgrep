use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode as StdExit;

use clap::Parser;
use globset::Glob;

use xgrep::config::{ColorChoice, LayerSet, OutputMode};
use xgrep::error::ExitCode;
use xgrep::matcher::{CaseMode, Pattern};
use xgrep::printer::print_block;
use xgrep::reader::ReaderOptions;
use xgrep::walker::walk_xlsx;
use xgrep::worker::run_search;
use xgrep::MatchEvent;

#[derive(Parser, Debug)]
#[command(
    name = "xgrep",
    version,
    about = "Excel-aware grep — search .xlsx files with rg-style UX",
    arg_required_else_help = true,
)]
struct Cli {
    /// Regex pattern.
    pattern: Option<String>,

    /// Paths to search (default: current directory).
    paths: Vec<PathBuf>,

    /// Additional patterns (rg `-e`).
    #[arg(short = 'e', long = "regexp")]
    regexp: Vec<String>,

    #[arg(short = 'i', long, help = "Case insensitive")]
    ignore_case: bool,
    #[arg(short = 's', long, help = "Case sensitive (overrides smart-case)")]
    case_sensitive: bool,
    #[arg(short = 'S', long, help = "Smart case (default)")]
    smart_case: bool,
    #[arg(short = 'F', long, help = "Treat PATTERN as a literal string")]
    fixed_strings: bool,
    #[arg(short = 'w', long, help = "Match whole words only")]
    word_regexp: bool,
    #[arg(short = 'v', long, help = "Invert match (emit non-matching cells)")]
    invert_match: bool,

    #[arg(short = 'c', long, help = "Print path:count per matching file")]
    count: bool,
    #[arg(short = 'l', long = "files-with-matches", help = "Print only paths of matching files")]
    files_with_matches: bool,
    #[arg(long, help = "Stream NDJSON events")]
    json: bool,
    #[arg(long, value_enum, default_value_t = ColorOpt::Auto)]
    color: ColorOpt,
    #[arg(short = 'j', long = "threads", default_value_t = 0,
          help = "Worker threads (0 = number of CPUs)")]
    threads: usize,
    #[arg(long, help = "Glob to filter file paths, e.g. --glob 'reports/**/*.xlsx'")]
    glob: Option<String>,

    #[arg(long, help = "Search formula text (e.g. =SUM(...)) too")]
    formula: bool,
    #[arg(long = "no-hidden", help = "Skip hidden sheets/rows/cols")]
    no_hidden: bool,
    #[arg(long = "no-comments", help = "Skip cell comments")]
    no_comments: bool,
    #[arg(long, help = "Glob over sheet names, e.g. --sheet 'Q*'")]
    sheet: Option<String>,
    #[arg(long, help = "Always print the layer tag (even [display])")]
    layers: bool,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum ColorOpt { Auto, Always, Never }

impl From<ColorOpt> for ColorChoice {
    fn from(c: ColorOpt) -> Self {
        match c {
            ColorOpt::Auto => ColorChoice::Auto,
            ColorOpt::Always => ColorChoice::Always,
            ColorOpt::Never => ColorChoice::Never,
        }
    }
}

fn main() -> StdExit {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => StdExit::from(code.as_i32() as u8),
        Err(e) => {
            let _ = writeln!(io::stderr(), "xgrep: {e}");
            StdExit::from(ExitCode::Fatal.as_i32() as u8)
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    // Pattern: positional `pattern` + any number of `-e` flags. Combine as alternation.
    let patterns: Vec<String> = cli.regexp.iter().cloned()
        .chain(cli.pattern.clone())
        .collect();
    if patterns.is_empty() {
        anyhow::bail!("no pattern provided (PATTERN or -e/--regexp)");
    }
    let joined = if patterns.len() == 1 {
        patterns[0].clone()
    } else {
        format!("(?:{})", patterns.join(")|(?:"))
    };
    let case = if cli.ignore_case { CaseMode::Insensitive }
        else if cli.case_sensitive { CaseMode::Sensitive }
        else { CaseMode::Smart };
    let pattern = Pattern::compile(&joined, case, cli.fixed_strings, cli.word_regexp)
        .map_err(|e| anyhow::anyhow!("invalid regex: {e}"))?;

    let mut layers = LayerSet::DISPLAY | LayerSet::CACHED;
    if !cli.no_comments { layers |= LayerSet::COMMENT; }
    if cli.formula { layers |= LayerSet::FORMULA; }

    let paths_in = if cli.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else { cli.paths.clone() };
    let file_glob = cli.glob.as_deref().map(Glob::new).transpose()?;
    let sheet_glob = cli.sheet.as_deref().map(Glob::new).transpose()?;

    let xlsx_paths = walk_xlsx(&paths_in, file_glob.as_ref())?;
    if xlsx_paths.is_empty() {
        return Ok(ExitCode::NoMatch);
    }

    let reader_opts = ReaderOptions {
        layers,
        include_hidden: !cli.no_hidden,
        sheet_filter: sheet_glob.as_ref().map(|g| g.compile_matcher()),
    };
    let threads = if cli.threads == 0 { num_cpus().max(1) } else { cli.threads };
    let blocks = run_search(xlsx_paths, &pattern, &reader_opts, cli.invert_match, threads);

    let output = if cli.json { OutputMode::Json }
        else if cli.count { OutputMode::CountOnly }
        else if cli.files_with_matches { OutputMode::FilesOnly }
        else { OutputMode::Pretty };

    let color_choice: ColorChoice = cli.color.into();
    let effective_color = match color_choice {
        ColorChoice::Auto if io::stdout().is_terminal() => ColorChoice::Always,
        ColorChoice::Auto => ColorChoice::Never,
        c => c,
    };

    let mut total_matches = 0u64;
    let mut had_error = false;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for block in &blocks {
        for ev in &block.events {
            match ev {
                MatchEvent::Match { .. } => total_matches += 1,
                MatchEvent::Error { message, path } => {
                    had_error = true;
                    let _ = writeln!(io::stderr(), "xgrep: {}: {message}", path.display());
                }
                _ => {}
            }
        }
        if let Err(e) = print_block(block, &mut out, output, effective_color, cli.layers) {
            if e.kind() == io::ErrorKind::BrokenPipe {
                return Ok(ExitCode::Match);
            }
            return Err(e.into());
        }
    }
    let _ = out.flush();
    let _ = had_error; // not escalated to Fatal per spec §6.3

    Ok(ExitCode::from_outcome(total_matches, false))
}

fn num_cpus() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}
