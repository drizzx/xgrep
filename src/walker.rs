//! Path discovery. Uses `ignore::WalkBuilder` so we get standard `.gitignore`
//! semantics and high-throughput parallel iteration "for free". xlsx is filtered
//! by extension; an optional glob narrows further (matched against the full path).

use std::path::PathBuf;

use globset::Glob;
use ignore::WalkBuilder;

use crate::error::SearchError;

pub fn walk_xlsx(
    roots: &[PathBuf],
    file_glob: Option<&Glob>,
) -> Result<Vec<PathBuf>, SearchError> {
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let matcher = file_glob.map(|g| g.compile_matcher());
    let mut builder = WalkBuilder::new(&roots[0]);
    for r in &roots[1..] { builder.add(r); }
    builder.standard_filters(true);

    let mut out = Vec::new();
    for result in builder.build() {
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) { continue; }
        let path = entry.into_path();
        if path.extension().map(|e| e.eq_ignore_ascii_case("xlsx")).unwrap_or(false) {
            if let Some(m) = &matcher {
                if !m.is_match(&path) { continue; }
            }
            out.push(path);
        }
    }
    Ok(out)
}
