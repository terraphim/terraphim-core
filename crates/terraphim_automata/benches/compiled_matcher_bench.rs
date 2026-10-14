//! Benchmarks for the compiled matcher (`CompiledMatcher`).
//!
//! The `default` group exercises the leftmost-longest, word-boundary path that
//! existing callers rely on; it guards against regressions when matcher
//! options grow. The `positions_*` groups compare the allocation-free
//! positions API (`find_positions`, `push_positions`) against
//! `find_matches(text, true)`, which clones the pattern and term per match,
//! in both overlap modes (Refs #87). Run with
//! `cargo bench -p terraphim_automata --bench compiled_matcher_bench`.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use terraphim_automata::{CompiledMatcher, MatchOverlap, Matched, MatcherOptions, PositionedMatch};
use terraphim_types::{NormalizedTerm, NormalizedTermValue, Thesaurus};

const TERMS: &[&str] = &[
    "machine learning",
    "deep learning",
    "learning",
    "artificial intelligence",
    "intelligence",
    "neural network",
    "network",
    "knowledge graph",
    "graph",
    "rust",
    "rust async",
    "async runtime",
    "runtime",
    "natural language processing",
    "language",
    "search engine",
    "engine",
    "aho corasick",
    "automata",
    "thesaurus",
];

fn thesaurus() -> Thesaurus {
    let mut thesaurus = Thesaurus::new("bench".to_string());
    for (id, term) in TERMS.iter().enumerate() {
        thesaurus.insert(
            NormalizedTermValue::from(*term),
            NormalizedTerm::new(id as u64 + 1, NormalizedTermValue::from(*term)),
        );
    }
    thesaurus
}

fn corpus() -> String {
    let paragraph = "Machine learning and deep learning rely on neural network \
        models, while a knowledge graph gives artificial intelligence a \
        structured memory. The Rust async runtime powers a search engine built \
        on aho corasick automata and a curated thesaurus for natural language \
        processing. Words such as networking, graphite and engineered must not \
        match inside longer words. ";
    paragraph.repeat(64)
}

fn bench_default_mode(c: &mut Criterion) {
    let matcher = CompiledMatcher::from_thesaurus(&thesaurus(), MatcherOptions::default())
        .expect("bench thesaurus compiles");
    let text = corpus();

    let mut group = c.benchmark_group("compiled_matcher_default");
    group.throughput(Throughput::Bytes(text.len() as u64));
    group.bench_function("find_matches_positions", |b| {
        b.iter(|| matcher.find_matches(black_box(&text), true).unwrap())
    });
    group.bench_function("push_matches_reused_buffer", |b| {
        let mut buffer: Vec<Matched> = Vec::new();
        b.iter(|| {
            buffer.clear();
            matcher.push_matches(black_box(&text), &mut buffer).unwrap();
            buffer.len()
        })
    });
    group.bench_function("find_iter_count", |b| {
        b.iter(|| matcher.find_iter(black_box(&text)).count())
    });
    group.finish();
}

fn bench_positions(c: &mut Criterion) {
    let text = corpus();
    for (name, overlap) in [
        ("positions_leftmost_longest", MatchOverlap::LeftmostLongest),
        ("positions_all", MatchOverlap::All),
    ] {
        let matcher = CompiledMatcher::from_thesaurus(
            &thesaurus(),
            MatcherOptions::default().with_overlap(overlap),
        )
        .expect("bench thesaurus compiles");

        let mut group = c.benchmark_group(name);
        group.throughput(Throughput::Bytes(text.len() as u64));
        group.bench_function("find_matches_positions", |b| {
            b.iter(|| matcher.find_matches(black_box(&text), true).unwrap().len())
        });
        group.bench_function("find_positions_count", |b| {
            b.iter(|| matcher.find_positions(black_box(&text)).count())
        });
        group.bench_function("push_positions_reused_buffer", |b| {
            let mut buffer: Vec<PositionedMatch> = Vec::new();
            b.iter(|| {
                buffer.clear();
                matcher.push_positions(black_box(&text), &mut buffer);
                buffer.len()
            })
        });
        group.finish();
    }
}

criterion_group!(benches, bench_default_mode, bench_positions);
criterion_main!(benches);
