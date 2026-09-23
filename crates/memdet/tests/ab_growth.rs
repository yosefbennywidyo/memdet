//! End-to-end check that [`BootReport`] attributes a per-clone RSS spike to the
//! right phase. Uses real process RSS (no `override_rss`), so the thresholds are
//! deliberately loose.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use memdet::BootReport;

const MIB: usize = 1024 * 1024;
const PAYLOAD: usize = 4 * MIB;
const COPIES: usize = 64;

#[derive(Clone)]
struct Heavy(#[allow(dead_code)] Vec<u8>);

#[test]
fn deep_clone_phase_dwarfs_the_arc_phase() {
    let report = BootReport::new();
    let heavy = Heavy(vec![0x5A; PAYLOAD]);

    // "buggy": COPIES deep clones held alive at once, like Router::layer cloning
    // the Extension payload once per route.
    let deep_clones = {
        let _phase = report.bracket("deep_clone_per_route");
        (0..COPIES).map(|_| heavy.clone()).collect::<Vec<_>>()
    };

    // "fixed": one Arc, COPIES pointer clones.
    let arc = Arc::new(heavy);
    let arc_clones = {
        let _phase = report.bracket("arc_clone_per_route");
        (0..COPIES).map(|_| Arc::clone(&arc)).collect::<Vec<_>>()
    };

    let phases = report.phases();
    let delta = |label: &str| {
        phases
            .iter()
            .find(|phase| phase.label == label)
            .unwrap_or_else(|| panic!("missing phase {label}"))
            .delta_bytes
    };
    let deep = delta("deep_clone_per_route");
    let arc_delta = delta("arc_clone_per_route");

    assert_eq!(report.worst().unwrap().label, "deep_clone_per_route");
    assert!(
        deep > (COPIES / 2) * PAYLOAD,
        "deep-clone phase should add ~{} MiB, got {} MiB",
        COPIES * PAYLOAD / MIB,
        deep / MIB
    );
    assert!(
        arc_delta < deep / 10,
        "arc phase ({} MiB) should be a small fraction of the deep-clone phase ({} MiB)",
        arc_delta / MIB,
        deep / MIB
    );

    drop(deep_clones);
    drop(arc_clones);
}
