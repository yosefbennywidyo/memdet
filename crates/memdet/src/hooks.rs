//! A process-global [`BootReport`] plus helpers for attributing RSS growth to
//! named boot phases. Enabled by the `hooks` feature (on by default).
//!
//! Traits like Loco's `Hooks` / `Initializer` cannot be wrapped transparently
//! from outside, so the pattern is: drop one line into each phase you care about.
//!
//! ```
//! # fn build_router() {}
//! # fn connect_pools() {}
//! fn after_context() {
//!     let _phase = memdet::hooks::phase("after_context");
//!     connect_pools();
//! }
//!
//! fn after_routes() {
//!     let _phase = memdet::hooks::phase_warn("after_routes", 128 * 1024 * 1024);
//!     build_router();
//! }
//!
//! // at the end of boot:
//! memdet::hooks::log_boot_summary();
//! ```

use std::sync::OnceLock;

use crate::report::BootReport;
use crate::rss::RssBracket;

static REPORT: OnceLock<BootReport> = OnceLock::new();

/// The process-global boot report. Created on first use.
#[must_use]
pub fn report() -> &'static BootReport {
    REPORT.get_or_init(BootReport::new)
}

/// Start a bracket for boot phase `label`, recorded into [`report`] on drop.
///
/// Bind it to a variable for the whole phase: `let _phase = phase("boot");`.
pub fn phase(label: &'static str) -> RssBracket {
    report().bracket(label)
}

/// Like [`phase`], but also emits a `warn` (with the `tracing` feature) when the
/// phase's RSS growth exceeds `warn_at_bytes`.
pub fn phase_warn(label: &'static str, warn_at_bytes: usize) -> RssBracket {
    report().bracket_warn(label, warn_at_bytes)
}

/// Render the global report's summary table.
#[must_use]
pub fn boot_summary() -> String {
    report().summary_table()
}

/// Emit [`boot_summary`] once: as a `tracing` info event with the `tracing`
/// feature, otherwise to stderr.
pub fn log_boot_summary() {
    let summary = boot_summary();
    #[cfg(feature = "tracing")]
    tracing::info!("\n{summary}");
    #[cfg(not(feature = "tracing"))]
    eprintln!("{summary}");
}

/// Assert that the RSS growth recorded for a named phase is within `max_mib` MiB.
///
/// The first argument is a [`BootReport`] (e.g. [`report()`](report) or a
/// `memdet-loco` report); the second is the phase label; the third is the limit.
/// Panics if the phase was never recorded, or if it exceeded the limit.
#[macro_export]
macro_rules! assert_hook_rss_delta {
    ($report:expr, $label:expr, $max_mib:expr $(,)?) => {{
        let label: &str = $label;
        let max_mib: usize = $max_mib;
        match ($report).phase_delta_bytes(label) {
            None => panic!("assert_hook_rss_delta!: no phase named {label:?} was recorded"),
            Some(delta) => assert!(
                delta <= max_mib.saturating_mul(1024 * 1024),
                "phase {label:?} grew RSS by {} MiB, over the {max_mib} MiB limit",
                delta / (1024 * 1024)
            ),
        }
    }};
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const MIB: usize = 1024 * 1024;

    #[test]
    fn global_report_is_shared() {
        let a = report();
        let b = report();
        assert!(std::ptr::eq(a, b));
    }

    #[test]
    fn assert_hook_rss_delta_reads_a_named_phase() {
        let report = BootReport::new();
        {
            let low = crate::override_rss(20 * MIB);
            let phase = report.bracket("startup");
            drop(low);
            let high = crate::override_rss(50 * MIB);
            drop(phase);
            drop(high);
        }
        // 30 MiB delta: within 64, over 8.
        assert_hook_rss_delta!(report, "startup", 64);

        let over = std::panic::catch_unwind(|| assert_hook_rss_delta!(report, "startup", 8));
        assert!(over.is_err());

        let missing = std::panic::catch_unwind(|| assert_hook_rss_delta!(report, "nope", 999));
        assert!(missing.is_err());
    }
}
