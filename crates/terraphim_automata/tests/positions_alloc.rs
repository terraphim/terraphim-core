//! Proves that `find_positions` and `push_positions` (into a buffer with
//! enough capacity) perform no heap allocation (Refs #87).
//!
//! A counting global allocator wraps the system allocator; this test binary
//! holds a single test so no other thread allocates while it counts.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use terraphim_automata::{MatchOverlap, MatcherBuilder, MatcherOptions, PositionedMatch};
use terraphim_types::{NormalizedTerm, NormalizedTermValue};

struct CountingAllocator;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every method delegates to `System`, which upholds the
// `GlobalAlloc` contract; the counter is a plain atomic side effect.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        // SAFETY: forwarded unchanged from the caller's contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded unchanged from the caller's contract.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        // SAFETY: forwarded unchanged from the caller's contract.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn allocations_during<R>(f: impl FnOnce() -> R) -> (usize, R) {
    let before = ALLOCATIONS.load(Ordering::SeqCst);
    let result = f();
    (ALLOCATIONS.load(Ordering::SeqCst) - before, result)
}

#[test]
fn positions_do_not_allocate() {
    let text = "Rust async runtime: a knowledge graph of graphs. ".repeat(64);
    for overlap in [MatchOverlap::LeftmostLongest, MatchOverlap::All] {
        for word_boundaries in [true, false] {
            let options = MatcherOptions::default()
                .with_overlap(overlap)
                .with_word_boundaries(word_boundaries);
            let mut builder = MatcherBuilder::new(options);
            for (id, word) in ["rust", "rust async", "async runtime", "runtime", "graph"]
                .iter()
                .enumerate()
            {
                builder
                    .insert(
                        (*word).to_string(),
                        NormalizedTerm::new(id as u64, NormalizedTermValue::from(*word)),
                    )
                    .unwrap();
            }
            let matcher = builder.build().unwrap();

            let (count, (n, checksum)) = allocations_during(|| {
                matcher
                    .find_positions(&text)
                    .fold((0usize, 0usize), |(n, sum), m| {
                        (n + 1, sum ^ m.start ^ m.end ^ m.pattern_index)
                    })
            });
            assert!(n > 0, "{overlap:?}/{word_boundaries}: no matches");
            assert_eq!(
                count, 0,
                "find_positions allocated, {overlap:?}/{word_boundaries} ({checksum})"
            );

            let mut buffer: Vec<PositionedMatch> = Vec::with_capacity(n);
            let (count, ()) = allocations_during(|| matcher.push_positions(&text, &mut buffer));
            assert_eq!(buffer.len(), n);
            assert_eq!(
                count, 0,
                "push_positions allocated, {overlap:?}/{word_boundaries}"
            );

            // Reusing the buffer for the next query stays allocation-free.
            let (count, ()) = allocations_during(|| {
                buffer.clear();
                matcher.push_positions(&text, &mut buffer);
            });
            assert_eq!(count, 0, "reused buffer allocated");

            // Whereas find_matches allocates per match.
            let (count, matches) = allocations_during(|| matcher.find_matches(&text, true));
            assert_eq!(matches.unwrap().len(), n);
            assert!(count > n, "find_matches allocated only {count} times");
        }
    }
}
