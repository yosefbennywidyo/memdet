//! Boot-memory regression assertions. Enabled by the `testkit` feature.
//!
//! Two complementary checks:
//!
//! * [`with_dhat`] + [`assert_boot_heap!`](crate::assert_boot_heap) measure the
//!   heap your code *requested* (deterministic, cross-platform). Rely on this one
//!   as the budget check.
//! * [`measure_rss`] / [`measure_rss_async`] +
//!   [`assert_boot_rss!`](crate::assert_boot_rss) sample the process *resident
//!   set* over the run. That catches allocator retention and fragmentation, but
//!   it is noisy and platform-dependent for absolute numbers.
//!
//! ## RSS is noisy: assert on a percentile, not the max
//!
//! [`measure_rss`] returns an [`RssSamples`] distribution, not a single number.
//! The single highest reading is the noisiest thing you can pick; assert on
//! [`RssSamples::p95`] or [`RssSamples::p99`] instead, with generous slack, and
//! only gate absolute numbers on Linux.
//!
//! ## Using `with_dhat`
//!
//! The test binary must install dhat's allocator once, at crate root:
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: dhat::Alloc = dhat::Alloc;
//! ```
//!
//! dhat allows only one profiler at a time, so every test that calls
//! [`with_dhat`] must be serialised (e.g. `#[serial_test::serial]`).

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::rss::rss_now;

/// RSS polling interval for the samplers. Finer means better tail estimates at a
/// negligible cost (one `/proc/self/statm` read is ~1 microsecond).
const SAMPLE_INTERVAL: Duration = Duration::from_millis(2);

/// Run `f` under a dhat heap profiler and return its result together with the
/// heap statistics gathered while it ran.
///
/// Requires the test binary to set `dhat::Alloc` as its global allocator (see the
/// module docs). Serialise callers: dhat permits only one profiler at a time.
#[must_use]
pub fn with_dhat<T>(f: impl FnOnce() -> T) -> (T, dhat::HeapStats) {
    let profiler = dhat::Profiler::builder().testing().build();
    let out = f();
    let stats = dhat::HeapStats::get();
    drop(profiler);
    (out, stats)
}

/// Run `f` while polling the process RSS on a background thread; return its
/// result and the [`RssSamples`] observed.
#[must_use]
pub fn measure_rss<T>(f: impl FnOnce() -> T) -> (T, RssSamples) {
    let sampler = RssSampler::start(SAMPLE_INTERVAL);
    let out = f();
    (out, sampler.finish())
}

/// Async counterpart of [`measure_rss`]. The sampler runs on its own OS thread,
/// so it keeps polling even while the future is blocked.
#[must_use]
pub async fn measure_rss_async<F, Fut, T>(f: F) -> (T, RssSamples)
where
    F: FnOnce() -> Fut,
    Fut: core::future::Future<Output = T>,
{
    let sampler = RssSampler::start(SAMPLE_INTERVAL);
    let out = f().await;
    (out, sampler.finish())
}

/// A sorted set of process-RSS readings (bytes) taken over one measured run.
///
/// Prefer [`p95`](Self::p95) / [`p99`](Self::p99) over [`max`](Self::max) for
/// assertions: the maximum is a single sample and swings the most between runs.
#[derive(Debug, Clone)]
pub struct RssSamples {
    /// Ascending.
    sorted: Vec<usize>,
}

impl RssSamples {
    fn new(mut samples: Vec<usize>) -> Self {
        samples.sort_unstable();
        Self { sorted: samples }
    }

    /// Number of readings taken.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sorted.len()
    }

    /// Whether no readings were taken.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sorted.is_empty()
    }

    /// The readings, ascending.
    #[must_use]
    pub fn as_slice(&self) -> &[usize] {
        &self.sorted
    }

    /// Lowest reading, or `0` if empty.
    #[must_use]
    pub fn min(&self) -> usize {
        self.sorted.first().copied().unwrap_or(0)
    }

    /// Highest reading, or `0` if empty. Noisy; prefer a percentile.
    #[must_use]
    pub fn max(&self) -> usize {
        self.sorted.last().copied().unwrap_or(0)
    }

    /// Arithmetic mean, or `0` if empty.
    #[must_use]
    pub fn mean(&self) -> usize {
        if self.sorted.is_empty() {
            0
        } else {
            self.sorted.iter().sum::<usize>() / self.sorted.len()
        }
    }

    /// The `p`th percentile (`0..=100`) by the nearest-rank method, or `0` if
    /// empty. Integer arithmetic only.
    #[must_use]
    pub fn percentile(&self, p: u8) -> usize {
        let n = self.sorted.len();
        if n == 0 {
            return 0;
        }
        let p = (p.min(100)) as usize;
        // rank = ceil(p * n / 100), 1-based
        let rank = (p * n).div_ceil(100);
        let idx = rank.saturating_sub(1).min(n - 1);
        self.sorted[idx]
    }

    /// 50th percentile (median).
    #[must_use]
    pub fn p50(&self) -> usize {
        self.percentile(50)
    }

    /// Alias for [`p50`](Self::p50).
    #[must_use]
    pub fn median(&self) -> usize {
        self.p50()
    }

    /// 90th percentile.
    #[must_use]
    pub fn p90(&self) -> usize {
        self.percentile(90)
    }

    /// 95th percentile.
    #[must_use]
    pub fn p95(&self) -> usize {
        self.percentile(95)
    }

    /// 99th percentile.
    #[must_use]
    pub fn p99(&self) -> usize {
        self.percentile(99)
    }
}

impl fmt::Display for RssSamples {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "rss n={} min={} p50={} p90={} p95={} p99={} max={} (MiB)",
            self.len(),
            mib_str(self.min()),
            mib_str(self.p50()),
            mib_str(self.p90()),
            mib_str(self.p95()),
            mib_str(self.p99()),
            mib_str(self.max()),
        )
    }
}

/// `bytes` as a `whole.frac` MiB string, integer arithmetic only, so the macros
/// never expand `as f64` into a caller's crate.
fn mib_str(bytes: usize) -> String {
    const MIB: usize = 1024 * 1024;
    let whole = bytes / MIB;
    let frac = (bytes % MIB) * 100 / MIB;
    format!("{whole}.{frac:02}")
}

/// Assertion message for "over the limit", built via [`mib_str`].
#[doc(hidden)]
#[must_use]
pub fn __limit_msg(what: &str, actual_bytes: usize, max_mib: usize) -> String {
    format!(
        "{what} {} MiB exceeds limit {max_mib} MiB",
        mib_str(actual_bytes)
    )
}

/// Assert that a heap high-water mark (bytes, e.g. `dhat::HeapStats::max_bytes`)
/// is within `max_mib` MiB. Panics with the actual figure on failure.
#[macro_export]
macro_rules! assert_boot_heap {
    ($actual_bytes:expr, $max_mib:expr $(,)?) => {{
        let actual: usize = $actual_bytes;
        let max_mib: usize = $max_mib;
        assert!(
            actual <= max_mib.saturating_mul(1024 * 1024),
            "{}",
            $crate::testkit::__limit_msg("boot heap high-water", actual, max_mib)
        );
    }};
}

/// Assert that an RSS reading (bytes) is within `max_mib` MiB. Pass a percentile,
/// e.g. `assert_boot_rss!(rss.p95(), 600)`; RSS is noisy, so keep the limit
/// generous.
#[macro_export]
macro_rules! assert_boot_rss {
    ($rss_bytes:expr, $max_mib:expr $(,)?) => {{
        let actual: usize = $rss_bytes;
        let max_mib: usize = $max_mib;
        assert!(
            actual <= max_mib.saturating_mul(1024 * 1024),
            "{} (RSS is noisy; widen the slack or use a lower percentile if this flaps)",
            $crate::testkit::__limit_msg("boot RSS", actual, max_mib)
        );
    }};
}

/// Polls `rss_now()` on a background thread, keeping every reading.
struct RssSampler {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<Vec<usize>>>,
}

impl RssSampler {
    fn start(interval: Duration) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let handle = {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut samples = vec![rss_now()];
                while !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(interval);
                    samples.push(rss_now());
                }
                samples.push(rss_now());
                samples
            })
        };
        Self {
            stop,
            handle: Some(handle),
        }
    }

    fn finish(mut self) -> RssSamples {
        self.stop.store(true, Ordering::Relaxed);
        let samples = self
            .handle
            .take()
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();
        RssSamples::new(samples)
    }
}

impl Drop for RssSampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const MIB: usize = 1024 * 1024;

    #[test]
    fn percentiles_use_nearest_rank() {
        let samples = RssSamples::new((1..=100).map(|x| x * MIB).collect());
        assert_eq!(samples.len(), 100);
        assert_eq!(samples.min(), MIB);
        assert_eq!(samples.max(), 100 * MIB);
        assert_eq!(samples.p50(), 50 * MIB);
        assert_eq!(samples.median(), samples.p50());
        assert_eq!(samples.p90(), 90 * MIB);
        assert_eq!(samples.p95(), 95 * MIB);
        assert_eq!(samples.p99(), 99 * MIB);
        assert_eq!(samples.mean(), 50 * MIB + MIB / 2);
    }

    #[test]
    fn empty_samples_are_all_zero() {
        let samples = RssSamples::new(Vec::new());
        assert!(samples.is_empty());
        assert_eq!(samples.p95(), 0);
        assert_eq!(samples.max(), 0);
        assert_eq!(samples.mean(), 0);
    }

    #[test]
    fn percentile_is_robust_to_a_single_spike() {
        // 199 low readings, one 10x spike: max jumps, p50/p95/p99 do not.
        let mut data = vec![10 * MIB; 199];
        data.push(100 * MIB);
        let samples = RssSamples::new(data);
        assert_eq!(samples.max(), 100 * MIB);
        assert_eq!(samples.p50(), 10 * MIB);
        assert_eq!(samples.p95(), 10 * MIB);
        assert_eq!(samples.p99(), 10 * MIB);
    }

    #[test]
    fn measure_rss_sees_a_transient_allocation() {
        let (_out, rss) = measure_rss(|| {
            let big = vec![0u8; 96 * MIB];
            std::hint::black_box(&big);
            std::thread::sleep(Duration::from_millis(30));
        });
        assert!(rss.len() >= 2, "expected several samples, got {rss}");
        assert!(rss.max() >= 64 * MIB, "expected >=64 MiB peak, got {rss}");
    }

    #[tokio::test]
    async fn async_sampler_also_works() {
        let (_out, rss) = measure_rss_async(|| async {
            let big = vec![7u8; 64 * MIB];
            std::hint::black_box(&big);
            tokio::time::sleep(Duration::from_millis(30)).await;
        })
        .await;
        assert!(rss.max() >= 32 * MIB, "got {rss}");
    }

    #[test]
    fn limit_msg_is_float_free_and_readable() {
        let msg = __limit_msg("boot RSS", 256 * MIB + MIB / 2, 64);
        assert_eq!(msg, "boot RSS 256.50 MiB exceeds limit 64 MiB");
    }

    #[test]
    fn assert_boot_rss_passes_under_limit() {
        crate::assert_boot_rss!(10 * MIB, 64);
    }

    #[test]
    #[should_panic(expected = "exceeds limit 8 MiB")]
    fn assert_boot_heap_panics_over_limit() {
        crate::assert_boot_heap!(32 * MIB, 8);
    }
}
