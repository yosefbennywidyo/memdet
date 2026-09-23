//! `testkit` end-to-end: the deterministic dhat heap budget catches the buggy
//! per-route clone, and passes the `Arc` variant. Also exercises the process-RSS
//! peak sampler.
//!
//! Run: `cargo test -p memdet --features testkit --test boot_regression`

#![cfg(feature = "testkit")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use memdet::testkit::{measure_rss, with_dhat};
use serial_test::serial;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

const MIB: usize = 1024 * 1024;
const PAYLOAD: usize = 4 * MIB;
const COPIES: usize = 64;
/// Budget between the two variants: Arc holds ~1 payload, deep-clone holds ~64.
const HEAP_BUDGET_MIB: usize = 32;

#[derive(Clone)]
struct Heavy(#[allow(dead_code)] Vec<u8>);

/// Mimics `Router::layer` cloning an `Extension` payload once per route.
fn build(arc: bool) -> Box<dyn std::any::Any> {
    let heavy = Heavy(vec![0x5A; PAYLOAD]);
    if arc {
        let shared = Arc::new(heavy);
        Box::new((0..COPIES).map(|_| Arc::clone(&shared)).collect::<Vec<_>>())
    } else {
        Box::new((0..COPIES).map(|_| heavy.clone()).collect::<Vec<_>>())
    }
}

#[test]
#[serial]
fn arc_variant_is_within_the_heap_budget() {
    let (_held, stats) = with_dhat(|| build(true));
    memdet::assert_boot_heap!(stats.max_bytes, HEAP_BUDGET_MIB);
}

#[test]
#[serial]
#[should_panic(expected = "exceeds limit 32 MiB")]
fn deep_clone_variant_blows_the_heap_budget() {
    let (_held, stats) = with_dhat(|| build(false));
    memdet::assert_boot_heap!(stats.max_bytes, HEAP_BUDGET_MIB);
}

#[test]
#[serial]
fn rss_distribution_reflects_the_deep_clone_variant() {
    let (_held, rss) = measure_rss(|| build(false));
    // ~256 MiB is added; assert well clear of noise, well under a loose ceiling.
    assert!(
        rss.p95() > 128 * MIB,
        "expected p95 RSS >128 MiB, got {rss}"
    );
    assert!(
        rss.p50() <= rss.p95() && rss.p95() <= rss.max(),
        "ordering: {rss}"
    );
    memdet::assert_boot_rss!(rss.p95(), 2048);
}
