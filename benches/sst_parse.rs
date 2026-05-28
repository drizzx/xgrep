//! Micro-bench for sst::parse_with_early_abort — gates the v0.2.1 perf hotfix.
//! Run: `cargo bench --bench sst_parse`. No fixture file required; sst XML is
//! synthesized in-memory.
//!
//! Gate: `heavy_sparse` median ≤ 30 ms (vs ~100 ms for the regex impl this
//! replaces).

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use xgrep::matcher::{CaseMode, Pattern};

const ABORT_THRESHOLD: usize = 100;

/// Build a synthetic sst XML string: `entries` total <si> nodes; the first
/// `hits` of them contain "HIT" (matched by the bench pattern), the rest
/// contain "miss". Order interleaves hits among misses for realism — hits
/// are at positions 0, step, 2*step, ... where step = entries / hits.
fn build_sst(entries: usize, hits: usize) -> String {
    let mut s = String::with_capacity(entries * 30);
    s.push_str("<sst>");
    let step = if hits == 0 { entries + 1 } else { entries / hits.max(1) };
    for i in 0..entries {
        if hits > 0 && i % step == 0 && (i / step) < hits {
            s.push_str(&format!("<si><t>HIT-row-{i}</t></si>"));
        } else {
            s.push_str(&format!("<si><t>miss-{i}</t></si>"));
        }
    }
    s.push_str("</sst>");
    s
}

/// Call site mirrors `src/reader/sst.rs::parse_xml_with_early_abort` but for
/// a pre-built `&str`. We can't call the private fn directly, so we
/// recreate the minimal harness here. The actual code under bench is the
/// xml_scan path through the public `parse` and a manual hit count loop.
/// Use `Pattern::is_match` to mirror the abort logic.
fn bench_parse(xml: &str, pat: &Pattern, threshold: usize) -> (usize, bool) {
    use std::ops::ControlFlow;
    let mut count = 0usize;
    let mut entries = 0usize;
    let aborted = xgrep::reader::xml_scan::for_each_tag(
        xml.as_bytes(),
        "si",
        |_attrs, body| {
            entries += 1;
            let mut text = String::new();
            xgrep::reader::xml_scan::for_each_tag(body, "t", |_a, t_body| {
                text.push_str(&xgrep::reader::xml_scan::xml_unescape(t_body));
                ControlFlow::Continue(())
            });
            if pat.is_match(&text) {
                count += 1;
                if count > threshold {
                    return ControlFlow::Break(());
                }
            }
            ControlFlow::Continue(())
        },
    );
    (count, aborted)
}

fn pattern_hit() -> Pattern {
    Pattern::compile("HIT", CaseMode::Sensitive, true, false).unwrap()
}

fn benches(c: &mut Criterion) {
    let pat = pattern_hit();
    let cases: &[(&str, usize, usize)] = &[
        // (label,           entries, hits)
        ("small_sparse",      5_000,    5),  // 0.001 density
        ("heavy_sparse",     50_000,   50),  // 0.001 density — micro-gate
        ("heavy_dense",      50_000, 25_000), // 0.5 density — abort triggers
        ("huge_sparse",     100_000,  100),  // 0.001 density
    ];

    let mut g = c.benchmark_group("sst_parse");
    for (label, entries, hits) in cases {
        let xml = build_sst(*entries, *hits);
        let id = BenchmarkId::new("sst_parse", label);
        g.bench_with_input(id, &xml, |b, xml| {
            b.iter(|| {
                let (count, aborted) = bench_parse(xml, &pat, ABORT_THRESHOLD);
                black_box((count, aborted));
            });
        });
    }
    g.finish();
}

criterion_group!(group, benches);
criterion_main!(group);
