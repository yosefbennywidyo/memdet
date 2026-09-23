//! Reading process RSS and the [`RssBracket`] scope guard.

use crate::report::{BootReport, Phase};

/// Read the current process resident set size (RSS), in bytes.
///
/// Returns `0` when the platform or build cannot report it (see
/// [`try_rss_now`] for the fallible form). This never fails so that it can be
/// called from a `Drop` implementation.
#[must_use]
pub fn rss_now() -> usize {
    #[cfg(any(test, feature = "test-util"))]
    {
        if let Some(bytes) = test_util::RSS_OVERRIDE.with(std::cell::Cell::get) {
            return bytes;
        }
    }
    try_rss_now().unwrap_or(0)
}

/// Read the current process RSS in bytes, or `None` if this platform or build
/// cannot report it.
#[must_use]
pub fn try_rss_now() -> Option<usize> {
    memory_stats::memory_stats().map(|stats| stats.physical_mem)
}

/// A scope guard that records how much the process RSS grew over its lifetime.
///
/// Create one at the start of a boot phase and let it drop at the end. On drop
/// it computes `rss_now() - start`, records the sample into its [`BootReport`]
/// if it has one, and (with the `tracing` feature) emits a `debug` event, or a
/// `warn` event when the growth exceeds the [`start_warn`](Self::start_warn)
/// threshold.
#[derive(Debug)]
#[must_use = "dropping the bracket immediately ends the measurement; bind it to a variable"]
pub struct RssBracket {
    label: &'static str,
    start: usize,
    warn_at: Option<usize>,
    sink: Option<BootReport>,
}

impl RssBracket {
    /// Start a standalone bracket labelled `label`. The sample is only logged,
    /// not recorded anywhere; use [`BootReport::bracket`] to collect it.
    pub fn start(label: &'static str) -> Self {
        Self {
            label,
            start: rss_now(),
            warn_at: None,
            sink: None,
        }
    }

    /// Start a standalone bracket that also emits a `warn` event (with the
    /// `tracing` feature) when the RSS growth exceeds `warn_at_bytes`.
    pub fn start_warn(label: &'static str, warn_at_bytes: usize) -> Self {
        Self {
            label,
            start: rss_now(),
            warn_at: Some(warn_at_bytes),
            sink: None,
        }
    }

    pub(crate) fn with_sink(mut self, report: BootReport) -> Self {
        self.sink = Some(report);
        self
    }
}

impl Drop for RssBracket {
    fn drop(&mut self) {
        let end = rss_now();
        let delta = end.saturating_sub(self.start);
        let over_threshold = self.warn_at.is_some_and(|limit| delta > limit);

        if let Some(sink) = &self.sink {
            sink.record(Phase {
                label: self.label,
                start_bytes: self.start,
                end_bytes: end,
                delta_bytes: delta,
                over_threshold,
            });
        }

        #[cfg(feature = "tracing")]
        if over_threshold {
            tracing::warn!(
                phase = self.label,
                start_bytes = self.start,
                end_bytes = end,
                delta_bytes = delta,
                warn_at_bytes = self.warn_at,
                "memdet: boot phase exceeded its RSS threshold"
            );
        } else {
            tracing::debug!(
                phase = self.label,
                start_bytes = self.start,
                end_bytes = end,
                delta_bytes = delta,
                "memdet: boot phase"
            );
        }
    }
}

#[cfg(any(test, feature = "test-util"))]
pub use test_util::{override_rss, RssOverrideGuard};

#[cfg(any(test, feature = "test-util"))]
mod test_util {
    use std::cell::Cell;

    thread_local! {
        pub(crate) static RSS_OVERRIDE: Cell<Option<usize>> = const { Cell::new(None) };
    }

    /// RAII guard from [`override_rss`]; restores the previous override on drop.
    #[derive(Debug)]
    #[must_use = "the override is cleared when the guard is dropped"]
    pub struct RssOverrideGuard(Option<usize>);

    impl Drop for RssOverrideGuard {
        fn drop(&mut self) {
            RSS_OVERRIDE.with(|cell| cell.set(self.0));
        }
    }

    /// Force [`rss_now`](crate::rss_now) to return `bytes` on the current thread
    /// until the returned guard is dropped. Test support only; available under
    /// `#[cfg(test)]` or the `test-util` feature.
    pub fn override_rss(bytes: usize) -> RssOverrideGuard {
        let previous = RSS_OVERRIDE.with(|cell| cell.replace(Some(bytes)));
        RssOverrideGuard(previous)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::BootReport;

    const MIB: usize = 1024 * 1024;

    #[test]
    fn standalone_bracket_does_not_record() {
        let _override = override_rss(50 * MIB);
        let bracket = RssBracket::start("solo");
        drop(bracket);
        // Nothing to assert beyond "no panic"; a standalone bracket has no sink.
    }

    #[test]
    fn bracket_records_growth_into_report() {
        let report = BootReport::new();

        let low = override_rss(100 * MIB);
        let phase = report.bracket("build_router");
        drop(low);

        let high = override_rss(180 * MIB);
        drop(phase);
        drop(high);

        let phases = report.phases();
        assert_eq!(phases.len(), 1);
        assert_eq!(phases[0].label, "build_router");
        assert_eq!(phases[0].start_bytes, 100 * MIB);
        assert_eq!(phases[0].end_bytes, 180 * MIB);
        assert_eq!(phases[0].delta_bytes, 80 * MIB);
        assert!(!phases[0].over_threshold);
    }

    #[test]
    fn shrinking_rss_records_zero_delta() {
        let report = BootReport::new();

        let high = override_rss(200 * MIB);
        let phase = report.bracket("teardown");
        drop(high);

        let low = override_rss(120 * MIB);
        drop(phase);
        drop(low);

        assert_eq!(report.phases()[0].delta_bytes, 0);
    }

    #[test]
    fn warn_threshold_marks_phase_over() {
        let report = BootReport::new();

        let low = override_rss(10 * MIB);
        let phase = report.bracket_warn("i18n_bundles", 5 * MIB);
        drop(low);

        let high = override_rss(20 * MIB);
        drop(phase);
        drop(high);

        assert!(report.phases()[0].over_threshold);
    }

    #[test]
    fn override_guard_restores_previous_value() {
        let outer = override_rss(MIB);
        assert_eq!(rss_now(), MIB);
        {
            let _inner = override_rss(2 * MIB);
            assert_eq!(rss_now(), 2 * MIB);
        }
        assert_eq!(rss_now(), MIB);
        drop(outer);
    }
}
