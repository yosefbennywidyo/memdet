//! [`BootReport`] and the [`Phase`] samples it collects.

use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use crate::rss::RssBracket;

/// One measured boot phase: an [`RssBracket`]'s label and the RSS it saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Phase {
    /// The bracket's label.
    pub label: &'static str,
    /// Process RSS in bytes when the bracket started.
    pub start_bytes: usize,
    /// Process RSS in bytes when the bracket dropped.
    pub end_bytes: usize,
    /// `end_bytes - start_bytes`, saturating at `0` (a phase that freed memory
    /// records `0`, not a negative number).
    pub delta_bytes: usize,
    /// `true` if `delta_bytes` exceeded the bracket's `start_warn` threshold.
    pub over_threshold: bool,
}

/// Collects [`Phase`] samples from the brackets it hands out and renders a
/// summary.
///
/// Cheap to clone: it is a handle to shared storage, so the copy a
/// [`bracket`](Self::bracket) keeps and the copy you read from are the same
/// list.
#[derive(Debug, Clone, Default)]
pub struct BootReport {
    phases: Arc<Mutex<Vec<Phase>>>,
}

impl BootReport {
    /// Create an empty report.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Hand out a bracket labelled `label` whose sample is recorded into this
    /// report when it drops.
    pub fn bracket(&self, label: &'static str) -> RssBracket {
        RssBracket::start(label).with_sink(self.clone())
    }

    /// Like [`bracket`](Self::bracket), but the bracket also emits a `warn`
    /// event (with the `tracing` feature) when its RSS growth exceeds
    /// `warn_at_bytes`.
    pub fn bracket_warn(&self, label: &'static str, warn_at_bytes: usize) -> RssBracket {
        RssBracket::start_warn(label, warn_at_bytes).with_sink(self.clone())
    }

    pub(crate) fn record(&self, phase: Phase) {
        // A poisoned lock means a bracket panicked mid-drop; drop the sample
        // rather than turn that into a second panic.
        if let Ok(mut phases) = self.phases.lock() {
            phases.push(phase);
        }
    }

    /// A snapshot of every recorded phase, in the order they were recorded.
    ///
    /// Returns an owned `Vec` because the phases live behind a lock.
    #[must_use]
    pub fn phases(&self) -> Vec<Phase> {
        self.phases
            .lock()
            .map_or_else(|_| Vec::new(), |phases| phases.clone())
    }

    /// Sum of every phase's `delta_bytes`.
    #[must_use]
    pub fn total_delta_bytes(&self) -> usize {
        self.phases().iter().map(|phase| phase.delta_bytes).sum()
    }

    /// The phase with the largest `delta_bytes`, if any were recorded.
    #[must_use]
    pub fn worst(&self) -> Option<Phase> {
        self.phases()
            .into_iter()
            .max_by_key(|phase| phase.delta_bytes)
    }

    /// The most recently recorded phase whose label is `label`, if any.
    #[must_use]
    pub fn phase_named(&self, label: &str) -> Option<Phase> {
        self.phases()
            .into_iter()
            .rev()
            .find(|phase| phase.label == label)
    }

    /// `phase_named(label).delta_bytes`, for assertions.
    #[must_use]
    pub fn phase_delta_bytes(&self, label: &str) -> Option<usize> {
        self.phase_named(label).map(|phase| phase.delta_bytes)
    }

    /// Record a phase from two RSS readings taken by the caller (bytes).
    ///
    /// Use this when an [`RssBracket`](crate::RssBracket) cannot span the region,
    /// e.g. across two separate `async` trait-method calls. `over_threshold` is
    /// always `false` for spans recorded this way.
    pub fn record_span(&self, label: &'static str, start_bytes: usize, end_bytes: usize) {
        self.record(Phase {
            label,
            start_bytes,
            end_bytes,
            delta_bytes: end_bytes.saturating_sub(start_bytes),
            over_threshold: false,
        });
    }

    /// A plain-text table of every phase, largest growth first, with the worst
    /// phase flagged. Intended to be printed once after boot.
    #[must_use]
    pub fn summary_table(&self) -> String {
        let mut phases = self.phases();
        let total: usize = phases.iter().map(|phase| phase.delta_bytes).sum();
        let worst_delta = phases
            .iter()
            .map(|phase| phase.delta_bytes)
            .max()
            .unwrap_or(0);
        phases.sort_by_key(|phase| std::cmp::Reverse(phase.delta_bytes));

        let mut out = String::new();
        let _ = writeln!(
            out,
            "memdet boot report - total {:+.1} MiB across {} phase(s)",
            mib(total),
            phases.len()
        );
        let _ = writeln!(
            out,
            "  {:<24} {:>12} {:>12} {:>14}",
            "phase", "start", "end", "delta"
        );
        for phase in &phases {
            let worst = if phase.delta_bytes == worst_delta && worst_delta > 0 {
                "  <- worst"
            } else {
                ""
            };
            let over = if phase.over_threshold {
                " (over threshold)"
            } else {
                ""
            };
            let _ = writeln!(
                out,
                "  {:<24} {:>8.1} MiB {:>8.1} MiB {:>+10.1} MiB{worst}{over}",
                phase.label,
                mib(phase.start_bytes),
                mib(phase.end_bytes),
                mib(phase.delta_bytes),
            );
        }
        out
    }
}

#[allow(clippy::cast_precision_loss)] // display only; sub-MiB precision is irrelevant here
fn mib(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const MIB: usize = 1024 * 1024;

    fn phase(label: &'static str, start: usize, end: usize) -> Phase {
        Phase {
            label,
            start_bytes: start,
            end_bytes: end,
            delta_bytes: end.saturating_sub(start),
            over_threshold: false,
        }
    }

    #[test]
    fn worst_and_total_delta() {
        let report = BootReport::new();
        report.record(phase("a", 0, 10 * MIB));
        report.record(phase("b", 10 * MIB, 200 * MIB));
        report.record(phase("c", 200 * MIB, 205 * MIB));

        assert_eq!(report.worst().unwrap().label, "b");
        // 10 + 190 + 5
        assert_eq!(report.total_delta_bytes(), 205 * MIB);
    }

    #[test]
    fn worst_is_none_when_empty() {
        assert!(BootReport::new().worst().is_none());
    }

    #[test]
    fn summary_table_sorts_by_delta_and_flags_worst() {
        let report = BootReport::new();
        report.record(phase("small", 0, MIB));
        report.record(phase("huge", MIB, 500 * MIB));

        let table = report.summary_table();
        let huge = table
            .lines()
            .position(|line| line.contains("huge"))
            .unwrap();
        let small = table
            .lines()
            .position(|line| line.contains("small"))
            .unwrap();

        assert!(huge < small, "largest delta must come first:\n{table}");
        assert!(
            table.contains("<- worst"),
            "worst phase must be flagged:\n{table}"
        );
        assert!(table.contains("total +500.0 MiB"), "{table}");
    }

    #[test]
    fn clones_share_storage() {
        let report = BootReport::new();
        let clone = report.clone();
        clone.record(phase("x", 0, MIB));
        assert_eq!(report.phases().len(), 1);
    }
}
