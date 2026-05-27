use anyhow::{anyhow, Result};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().ok_or_else(|| anyhow!("usage: cargo xtask <gen-benches|list-fixtures|measure-memory>"))?;
    match cmd.as_str() {
        "gen-benches" => Err(anyhow!("not yet implemented (Task 4)")),
        "list-fixtures" => Err(anyhow!("not yet implemented (Task 6)")),
        "measure-memory" => Err(anyhow!("not yet implemented (Task 7)")),
        other => Err(anyhow!("unknown subcommand: {other}")),
    }
}
