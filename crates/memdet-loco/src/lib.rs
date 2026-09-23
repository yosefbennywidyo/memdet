#![doc = include_str!("../README.md")]

use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use axum::Router;
use loco_rs::app::{AppContext, Initializer};
use loco_rs::Result;

pub use memdet::hooks::{boot_summary, log_boot_summary, phase, phase_warn, report};
pub use memdet::{assert_hook_rss_delta, BootReport};

/// Default label for the phase [`MemdetInitializer`] records.
pub const DEFAULT_LABEL: &str = "initializers+routes";

/// A Loco [`Initializer`] that brackets boot-time RSS growth across the
/// initializer and route-build phase, then logs the [`memdet`] summary table.
///
/// List it **last** in your app's `Hooks::initializers()` so its `before_run`
/// captures the baseline right before the phase and its `after_routes` runs after
/// every other initializer:
///
/// ```ignore
/// async fn initializers(_ctx: &AppContext) -> Result<Vec<Box<dyn Initializer>>> {
///     Ok(vec![
///         Box::new(ViewEngineInitializer),
///         Box::new(memdet_loco::MemdetInitializer::new()), // last
///     ])
/// }
/// ```
///
/// After boot, assert against the global report:
///
/// ```ignore
/// memdet_loco::assert_hook_rss_delta!(memdet_loco::report(), "initializers+routes", 128);
/// ```
#[derive(Debug)]
pub struct MemdetInitializer {
    baseline: AtomicUsize,
    label: &'static str,
}

impl MemdetInitializer {
    /// A new initializer recording the [`DEFAULT_LABEL`] phase.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            baseline: AtomicUsize::new(0),
            label: DEFAULT_LABEL,
        }
    }

    /// A new initializer recording the phase under `label`.
    #[must_use]
    pub const fn labelled(label: &'static str) -> Self {
        Self {
            baseline: AtomicUsize::new(0),
            label,
        }
    }
}

impl Default for MemdetInitializer {
    fn default() -> Self {
        Self::new()
    }
}

impl MemdetInitializer {
    /// Capture the RSS baseline. Runs from `before_run`.
    fn mark_baseline(&self) {
        self.baseline.store(memdet::rss_now(), Ordering::Relaxed);
    }

    /// Record the span since [`mark_baseline`](Self::mark_baseline) and log the
    /// summary. Runs from `after_routes`.
    fn finish(&self) {
        let start = self.baseline.load(Ordering::Relaxed);
        report().record_span(self.label, start, memdet::rss_now());
        log_boot_summary();
    }
}

#[async_trait]
impl Initializer for MemdetInitializer {
    fn name(&self) -> String {
        "memdet".to_string()
    }

    async fn before_run(&self, _ctx: &AppContext) -> Result<()> {
        self.mark_baseline();
        Ok(())
    }

    async fn after_routes(&self, router: Router, _ctx: &AppContext) -> Result<Router> {
        self.finish();
        Ok(router)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: usize = 1024 * 1024;

    // Compile-time proof the impl satisfies the trait object bound Loco uses.
    const _: fn() = || {
        fn takes_boxed(_: Box<dyn Initializer>) {}
        takes_boxed(Box::new(MemdetInitializer::new()));
    };

    #[test]
    fn name_is_stable() {
        assert_eq!(MemdetInitializer::new().name(), "memdet");
        assert_eq!(MemdetInitializer::default().label, DEFAULT_LABEL);
        assert_eq!(MemdetInitializer::labelled("custom").label, "custom");
    }

    #[test]
    fn baseline_then_finish_records_the_span() {
        let init = MemdetInitializer::labelled("t_initializers");

        let low = memdet::override_rss(30 * MIB);
        init.mark_baseline();
        drop(low);

        let high = memdet::override_rss(90 * MIB);
        init.finish();
        drop(high);

        let delta = report().phase_delta_bytes("t_initializers");
        assert_eq!(delta, Some(60 * MIB));
        assert_hook_rss_delta!(report(), "t_initializers", 128);
    }
}
