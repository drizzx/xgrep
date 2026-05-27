use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone)]
pub struct FixtureSpec {
    pub name: String,
    #[serde(default)]
    pub rows: u32,
    #[serde(default = "one")]
    pub sheets: u32,
    #[serde(default)]
    pub shared_strings: u32,
    #[serde(default)]
    pub formula_pct: f32,
    #[serde(default)]
    pub inline_strings_pct: f32,
    #[serde(default)]
    pub hit_density: f32,
    #[serde(default)]
    pub files: u32,
    #[serde(default)]
    pub description: String,
}

fn one() -> u32 { 1 }

#[derive(Debug, Deserialize)]
struct FixturesFile {
    fixture: Vec<FixtureSpec>,
}

fn load_fixtures() -> Result<Vec<FixtureSpec>> {
    let manifest = env!("CARGO_MANIFEST_DIR");
    // xtask's manifest dir is xgrep/xtask; the fixtures.toml lives at xgrep/benches/.
    let path = Path::new(manifest).join("..").join("benches").join("fixtures.toml");
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read {}", path.display()))?;
    let parsed: FixturesFile = toml::from_str(&text)
        .with_context(|| format!("parse {}", path.display()))?;
    Ok(parsed.fixture)
}

fn out_root() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest).join("..").join("target").join("bench-fixtures")
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().ok_or_else(|| anyhow!(
        "usage: cargo xtask <gen-benches|list-fixtures|measure-memory>"
    ))?;
    match cmd.as_str() {
        "gen-benches" => cmd_gen_benches(),
        "list-fixtures" => cmd_list_fixtures(),
        "measure-memory" => cmd_measure_memory(args.collect()),
        other => bail!("unknown subcommand: {other}"),
    }
}

fn cmd_gen_benches() -> Result<()> {
    let fixtures = load_fixtures()?;
    println!("loaded {} fixtures", fixtures.len());
    // Real generation lands in Task 4/5.
    bail!("gen-benches generator not yet implemented (Task 4)")
}

fn cmd_list_fixtures() -> Result<()> {
    bail!("list-fixtures not yet implemented (Task 6)")
}

fn cmd_measure_memory(_args: Vec<String>) -> Result<()> {
    bail!("measure-memory not yet implemented (Task 7)")
}
