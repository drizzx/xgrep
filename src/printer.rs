//! Output sinks. Each call to `print_block` writes one file's events to the
//! provided writer atomically — callers should hold a single writer lock to
//! guarantee no other thread interleaves.

use std::io::{self, Write};

use crate::config::{ColorChoice, OutputMode};
use crate::{FileBlock, MatchEvent};

pub fn print_block(
    block: &FileBlock,
    out: &mut impl Write,
    mode: OutputMode,
    color: ColorChoice,
    force_layer_tag: bool,
) -> io::Result<()> {
    match mode {
        OutputMode::Pretty => print_pretty(block, out, color, force_layer_tag),
        OutputMode::Json => print_json(block, out),
        OutputMode::CountOnly => print_count(block, out),
        OutputMode::FilesOnly => print_files_with_matches(block, out),
    }
}

fn print_pretty(
    block: &FileBlock,
    out: &mut impl Write,
    color: ColorChoice,
    force_layer_tag: bool,
) -> io::Result<()> {
    use termcolor::{Ansi, Color, ColorSpec, NoColor, WriteColor};

    let has_match = block
        .events
        .iter()
        .any(|e| matches!(e, MatchEvent::Match { .. }));
    if !has_match {
        return Ok(());
    }

    fn render<W: WriteColor>(
        out: &mut W,
        block: &FileBlock,
        force_layer_tag: bool,
    ) -> io::Result<()> {
        let mut path_spec = ColorSpec::new();
        path_spec.set_fg(Some(Color::Magenta)).set_bold(true);

        let mut sheet_spec = ColorSpec::new();
        sheet_spec.set_fg(Some(Color::Blue));

        let mut cell_spec = ColorSpec::new();
        cell_spec.set_fg(Some(Color::Green));

        let mut tag_spec = ColorSpec::new();
        tag_spec.set_fg(Some(Color::Cyan));

        let mut match_spec = ColorSpec::new();
        match_spec.set_fg(Some(Color::Yellow)).set_bold(true);

        for ev in &block.events {
            match ev {
                MatchEvent::FileBegin { path } => {
                    out.set_color(&path_spec)?;
                    writeln!(out, "{}", path.display())?;
                    out.reset()?;
                }
                MatchEvent::Match {
                    sheet,
                    cell,
                    layer,
                    text,
                    submatches,
                    ..
                } => {
                    let offset = submatches.first().map(|s| s.start + 1).unwrap_or(1);
                    write!(out, "  ")?;
                    if !sheet.is_empty() {
                        out.set_color(&sheet_spec)?;
                        write!(out, "{sheet}")?;
                        out.reset()?;
                        write!(out, "!")?;
                    }
                    out.set_color(&cell_spec)?;
                    write!(out, "{cell}")?;
                    out.reset()?;
                    write!(out, ":{offset}: ")?;

                    // Highlight match substrings within text. submatches use char offsets.
                    let mut pos = 0;
                    for sm in submatches {
                        let start_b = char_index_to_byte(text, sm.start);
                        let end_b = char_index_to_byte(text, sm.end);
                        if start_b > pos {
                            out.write_all(&text.as_bytes()[pos..start_b])?;
                        }
                        out.set_color(&match_spec)?;
                        out.write_all(&text.as_bytes()[start_b..end_b])?;
                        out.reset()?;
                        pos = end_b;
                    }
                    if pos < text.len() {
                        out.write_all(&text.as_bytes()[pos..])?;
                    }

                    if layer != "display" || force_layer_tag {
                        out.set_color(&tag_spec)?;
                        write!(out, " [{}]", layer)?;
                        out.reset()?;
                    }
                    writeln!(out)?;
                }
                MatchEvent::Error { message, .. } => {
                    writeln!(out, "  ERROR: {message}")?;
                }
                MatchEvent::Context { sheet, cell, text, .. } => {
                    write!(out, "  ")?;
                    if !sheet.is_empty() {
                        out.set_color(&sheet_spec)?;
                        write!(out, "{sheet}")?;
                        out.reset()?;
                        write!(out, "!")?;
                    }
                    out.set_color(&cell_spec)?;
                    write!(out, "{cell}")?;
                    out.reset()?;
                    write!(out, ": {text} ")?;
                    out.set_color(&tag_spec)?;
                    write!(out, "[context]")?;
                    out.reset()?;
                    writeln!(out)?;
                }
                MatchEvent::Separator => {
                    writeln!(out, "  --")?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    match color {
        ColorChoice::Never => {
            let mut nc = NoColor::new(out);
            render(&mut nc, block, force_layer_tag)
        }
        ColorChoice::Always => {
            let mut ansi = Ansi::new(out);
            render(&mut ansi, block, force_layer_tag)
        }
        ColorChoice::Auto => {
            // Auto is resolved to Never/Always by main.rs before reaching here.
            // If it leaks through (e.g. in tests), default to no color.
            let mut nc = NoColor::new(out);
            render(&mut nc, block, force_layer_tag)
        }
    }
}

fn char_index_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

fn print_json(block: &FileBlock, out: &mut impl Write) -> io::Result<()> {
    for ev in &block.events {
        serde_json::to_writer(&mut *out, ev).map_err(io_other)?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

fn print_count(block: &FileBlock, out: &mut impl Write) -> io::Result<()> {
    let mut count = 0u64;
    let mut path = None;
    for ev in &block.events {
        match ev {
            MatchEvent::FileBegin { path: p } => path = Some(p.clone()),
            MatchEvent::Match { .. } => count += 1,
            _ => {}
        }
    }
    if let (Some(p), n) = (path, count) {
        if n > 0 {
            writeln!(out, "{}:{}", p.display(), n)?;
        }
    }
    Ok(())
}

fn print_files_with_matches(block: &FileBlock, out: &mut impl Write) -> io::Result<()> {
    let has_match = block
        .events
        .iter()
        .any(|e| matches!(e, MatchEvent::Match { .. }));
    if !has_match {
        return Ok(());
    }
    if let Some(MatchEvent::FileBegin { path }) = block.events.first() {
        writeln!(out, "{}", path.display())?;
    }
    Ok(())
}

fn io_other(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
