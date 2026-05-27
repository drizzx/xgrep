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

mod gen {
    use anyhow::{Context, Result};
    use rust_xlsxwriter::Workbook;
    use std::path::Path;

    use super::FixtureSpec;

    /// Build a single .xlsx file at `out` from `spec`. Files-mode (`spec.files > 0`)
    /// is handled by a separate path; this writes a single workbook.
    pub fn write_single(spec: &FixtureSpec, out: &Path) -> Result<()> {
        let mut wb = Workbook::new();
        let sst_size = if spec.shared_strings > 0 {
            spec.shared_strings
        } else {
            (spec.rows / 10).max(10)
        };
        // Pre-build the sst pool: each entry is "row-<idx>" by default; entries whose
        // index falls within `hit_density * sst_size` start with "HIT-" to give bench
        // patterns ("HIT") a controllable hit ratio.
        let hit_cut = ((sst_size as f32) * spec.hit_density) as u32;
        let sst_pool: Vec<String> = (0..sst_size)
            .map(|i| {
                if i < hit_cut {
                    format!("HIT-row-{i}")
                } else {
                    format!("row-{i}")
                }
            })
            .collect();

        for s in 0..spec.sheets {
            let sheet_name = format!("Sheet{}", s + 1);
            let ws = wb
                .add_worksheet()
                .set_name(&sheet_name)
                .with_context(|| format!("sheet name {sheet_name}"))?;
            for r in 0..spec.rows {
                // Column A: number (drives non-sst, non-inline-string scan path)
                ws.write_number(r, 0, (r as f64) * 1.5)
                    .with_context(|| format!("A{}", r + 1))?;
                // Column B: shared-string-typed cell (rust_xlsxwriter dedups via write_string)
                let s_idx = (r as usize) % sst_pool.len();
                let s_text = &sst_pool[s_idx];
                ws.write_string(r, 1, s_text)
                    .with_context(|| format!("B{}", r + 1))?;
                // Column C: optionally inline string (write_string_only bypasses sst when feature set)
                if spec.inline_strings_pct > 0.0 {
                    if ((r as f32) / (spec.rows as f32)) < spec.inline_strings_pct {
                        ws.write_string(r, 2, &format!("inline-{r}"))
                            .with_context(|| format!("C{}", r + 1))?;
                    }
                }
                // Column D: optional formula
                // Note: write_formula_with_result does not exist in rust_xlsxwriter 0.79.
                // Use write_formula + set_formula_result instead.
                if spec.formula_pct > 0.0 {
                    if ((r as f32) / (spec.rows as f32)) < spec.formula_pct {
                        ws.write_formula(r, 3, "=A1+B1")
                            .with_context(|| format!("D{}", r + 1))?;
                        ws.set_formula_result(r, 3, &format!("{:.1}", (r as f64) * 2.5));
                    }
                }
            }
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        wb.save(out)?;
        Ok(())
    }
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
    let root = out_root();
    std::fs::create_dir_all(&root)?;
    for f in &fixtures {
        if f.files > 0 {
            let dir = root.join(&f.name);
            std::fs::create_dir_all(&dir)?;
            for i in 0..f.files {
                let path = dir.join(format!("file_{i:03}.xlsx"));
                if path.exists() {
                    continue;
                }
                gen::write_single(f, &path)?;
            }
            println!("gen {} -> {} ({} files)", f.name, dir.display(), f.files);
            continue;
        }
        let path = root.join(format!("{}.xlsx", f.name));
        if path.exists() {
            println!("ok {} (cached)", f.name);
            continue;
        }
        println!("gen {} -> {}", f.name, path.display());
        gen::write_single(f, &path)?;
    }
    Ok(())
}

fn cmd_list_fixtures() -> Result<()> {
    bail!("list-fixtures not yet implemented (Task 6)")
}

fn cmd_measure_memory(_args: Vec<String>) -> Result<()> {
    bail!("measure-memory not yet implemented (Task 7)")
}
