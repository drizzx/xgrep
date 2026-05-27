//! v0.2 perf bench matrix. See docs/superpowers/specs/2026-05-28-xgrep-v0.2-design.md §4.4.
//! Run: `cargo bench --bench search`. Baseline diff: `--baseline v0.1`.
//! Fixtures must exist; run `cargo xtask gen-benches` first.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::path::{Path, PathBuf};
use xgrep::matcher::{CaseMode, Pattern};
use xgrep::reader::ReaderOptions;

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("bench-fixtures")
}

fn require_single(name: &str) -> PathBuf {
    let p = fixtures_root().join(format!("{name}.xlsx"));
    if !p.exists() {
        panic!(
            "fixture {name} missing at {} — run `cargo xtask gen-benches`",
            p.display()
        );
    }
    p
}

fn require_dir(name: &str) -> Vec<PathBuf> {
    let d = fixtures_root().join(name);
    if !d.exists() {
        panic!(
            "fixture dir {name} missing at {} — run `cargo xtask gen-benches`",
            d.display()
        );
    }
    let mut out: Vec<PathBuf> = std::fs::read_dir(&d)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("xlsx"))
        .collect();
    out.sort();
    out
}

fn search_one(path: &Path, pat: &Pattern, opts: &ReaderOptions) -> u64 {
    let block = xgrep::search_file(path, pat, opts, false);
    block
        .events
        .iter()
        .filter(|e| matches!(e, xgrep::MatchEvent::Match { .. }))
        .count() as u64
}

fn pattern_for(label: &str) -> Pattern {
    let (raw, case, fixed) = match label {
        "nonhit"  => ("ZZZ_NEVER_MATCH", CaseMode::Sensitive, true),
        "hit"     => ("HIT",             CaseMode::Sensitive, true),
        "regex"   => (r"row-\d{4,}",     CaseMode::Sensitive, false),
        other     => panic!("unknown pattern label {other}"),
    };
    Pattern::compile(raw, case, fixed, false).unwrap()
}

fn bench_single(c: &mut Criterion, fixture: &str, patterns: &[&str]) {
    let path = require_single(fixture);
    let opts = ReaderOptions::default();
    let mut g = c.benchmark_group(fixture);
    for label in patterns {
        let pat = pattern_for(label);
        g.bench_with_input(BenchmarkId::new(fixture, label), &path, |b, p| {
            b.iter(|| search_one(p, &pat, &opts));
        });
    }
    g.finish();
}

fn bench_many_small(c: &mut Criterion, patterns: &[&str]) {
    let paths = require_dir("many_small");
    let opts = ReaderOptions::default();
    let mut g = c.benchmark_group("many_small");
    for label in patterns {
        let pat = pattern_for(label);
        g.bench_with_input(BenchmarkId::new("many_small", label), &paths, |b, ps| {
            b.iter(|| {
                let mut total = 0u64;
                for p in ps {
                    total += search_one(p, &pat, &opts);
                }
                total
            });
        });
    }
    g.finish();
}

fn benches(c: &mut Criterion) {
    bench_single(c, "sst_heavy_low_hit",     &["nonhit", "hit", "regex"]);
    bench_single(c, "sst_heavy_high_hit",    &["hit", "regex", "nonhit"]);
    bench_single(c, "formula_heavy",         &["nonhit", "hit", "regex"]);
    bench_single(c, "inline_strings_heavy",  &["nonhit", "hit", "regex"]);
    bench_many_small(c, &["nonhit", "hit", "regex"]);
}

criterion_group!(group, benches);
criterion_main!(group);
