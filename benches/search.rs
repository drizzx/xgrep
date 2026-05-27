//! v0.2 perf bench harness. Operates on lib API directly, not the CLI binary,
//! to avoid process-spawn + stdout-serialization noise. Fixtures must be
//! generated first via `cargo xtask gen-benches`. Compare results against
//! `bench-baselines/v0.1/` (see plan Task 12).

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::path::PathBuf;

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("bench-fixtures")
}

fn require_fixture(name: &str) -> PathBuf {
    let p = fixtures_root().join(format!("{name}.xlsx"));
    if !p.exists() {
        panic!(
            "bench fixture {} not found at {}\nrun `cargo xtask gen-benches` first",
            name,
            p.display()
        );
    }
    p
}

fn bench_smoke(c: &mut Criterion) {
    // Smoke benchmark: just confirms the harness compiles + runs. Real matrix
    // populated in Task 13 once all five fixtures exist.
    let path = require_fixture("smoke");
    let mut g = c.benchmark_group("smoke");
    g.bench_with_input(BenchmarkId::new("smoke", "noop"), &path, |b, _p| {
        b.iter(|| 0_u64);
    });
    g.finish();
}

criterion_group!(benches, bench_smoke);
criterion_main!(benches);
