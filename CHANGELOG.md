# Changelog

All notable changes to this workspace are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this project aims to
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) once published.

## [Unreleased]

## [0.1.0-alpha] - 2026-09-23

First tagged version of `memdet` and `memdet-loco` (shared workspace version).
First release on crates.io.

### Changed

- `memdet` README rewritten for crates.io / docs.rs readers: problem statement,
  a compiled example with real `summary_table()` output, every feature documented,
  `hooks` and `testkit` usage, and the limits of RSS as a measure.
- Workspace version `0.1.0` → `0.1.0-alpha`; `memdet-loco` pins
  `memdet = "=0.1.0-alpha"`.
- `LICENSE-MIT` / `LICENSE-APACHE` added; docs.rs builds `memdet` with all features.

### Added — `memdet` core (part A)

- `rss_now()` / `try_rss_now()` — process RSS in bytes via `memory-stats`;
  `rss_now` never fails so it is safe to call from `Drop`.
- `RssBracket` — a scope guard that records how much RSS grew over its lifetime.
  `RssBracket::start` / `start_warn`; on drop it records into a `BootReport` (if
  attached) and, with the `tracing` feature, logs `debug`, or `warn` past the
  `start_warn` threshold.
- `BootReport` — a cheap-to-clone handle that collects `Phase` samples from the
  brackets it hands out (`bracket` / `bracket_warn`) and renders `summary_table()`
  (largest growth first, worst phase flagged), `worst()`, `total_delta_bytes()`.
- `override_rss()` test hook behind `#[cfg(any(test, feature = "test-util"))]`.

### Added — `axum` feature (part A)

- `memdet::axum::tracked_extension<T>(value) -> axum::Extension<T>` —
  `#[track_caller]`; with `tracing` it logs whether the payload looks `Arc`-shaped
  (`debug`) or not (`warn`, "`Router::layer` deep-clones it once per route").
- `examples/axum_ab.rs` — A/B demo: 128 routes, a 4 MiB `Clone` payload, buggy
  `layer(Extension(value))` vs fixed `layer(Extension(Arc::new(value)))`. The
  summary table flags the buggy router build at several hundred MiB (~+810 MiB
  on macOS arm64, 2026-09-23) and the fixed one at ~+0 MiB.
- `tests/ab_growth.rs` — real-RSS integration test of the same contrast.

### Added — `testkit` feature (part B)

- `memdet::testkit::with_dhat(f) -> (T, dhat::HeapStats)` — run `f` under a dhat
  heap profiler and return the heap high-water mark. The test binary must install
  `dhat::Alloc` as its global allocator; callers must be serialised.
- `measure_rss(f)` / `measure_rss_async(f)` — run `f` while an OS-thread sampler
  polls RSS every 2 ms, return an `RssSamples` distribution.
- `RssSamples` — `min` / `max` / `mean`, `percentile(p)`, `p50` / `p90` / `p95` /
  `p99`, `median` (alias of `p50`), `as_slice`, `len` / `is_empty`, `Display`
  (`rss n=… min=… p50=… p90=… p95=… p99=… max=… (MiB)`). Nearest-rank, integer
  arithmetic only. Assert on `p95` / `p99`, not `max` (a single, noisy sample).
- `assert_boot_heap!(bytes, max_mib)` and `assert_boot_rss!(bytes, max_mib)` —
  budget assertions with float-free failure messages carrying the actual figure;
  pass a percentile to `assert_boot_rss!`, e.g. `assert_boot_rss!(rss.p95(), 600)`.
- `tests/boot_regression.rs` — the deterministic dhat budget passes the `Arc`
  variant and fails the deep-clone variant; the RSS `p95` reflects the deep-clone.

### Added — `hooks` feature (part A, default on)

- `memdet::hooks::report()` — a process-global `BootReport` (`OnceLock`), so a
  phase started in one hook and read in another share storage.
- `phase(label)` / `phase_warn(label, bytes)` — `RssBracket`s bound to the global
  report; drop `let _p = memdet::hooks::phase("after_context");` into any hook.
- `boot_summary()` / `log_boot_summary()` — render / emit the summary table once
  after boot (`tracing` info, or stderr without the feature).
- `assert_hook_rss_delta!(report, "label", max_mib)` — assert a named phase's RSS
  growth; panics if the phase was never recorded.
- `BootReport::phase_named(label)` / `phase_delta_bytes(label)` / `record_span(
  label, start, end)` — look a phase up by label, or record one from two RSS
  readings you took yourself (for regions a bracket can't span, e.g. across two
  async trait-method calls).

### Added — `memdet-loco` crate

- `MemdetInitializer` — a `loco_rs::app::Initializer` (loco-rs 1.x, `default-features = false`). List it last
  in `Hooks::initializers()`: `before_run` records the RSS baseline, `after_routes`
  (invoked after every other initializer) records the delta as one
  `"initializers+routes"` phase and logs the summary. `new()` / `labelled(label)`
  / `Default`. No-op under `StartMode::WorkerOnly` (Loco skips `setup_routes`).
- Re-exports `memdet::hooks::*` and `assert_hook_rss_delta!`.
- Not a workspace default member — it pulls the `loco-rs` dependency tree; build with
  `cargo build -p memdet-loco`. New workspace deps: `loco-rs 1` (default-features off),
  `async-trait 0.1`. Targets loco 1.x.

### Added — `memdet-lints` crate (Dylint, part C)

Versioned separately (`0.1.0`, `publish = false`); not covered by the
`0.1.0-alpha` tag's version number.


- `extension_not_arc` — a `LateLintPass` that flags `axum::Extension(x)` where
  `x`'s type is not `Arc` / `Rc` (nor `Copy`). Matches the `Extension`
  tuple-struct constructor by crate (`axum`) + item name; `Arc`/`Rc` check via
  `clippy_utils::ty::should_call_clone_as_function`.
- Config via `dylint.toml` `[memdet-lints]`: `allow` (substrings for types that
  clone cheaply, e.g. `sqlx::Pool`) and `deny_heavy` (known-expensive → firmer
  help text).
- Its own `[workspace]` on pinned `nightly-2026-05-28` (dylint 6.0,
  `clippy_utils` git `rev 9fca3bc`); `exclude`d from the `memdet` workspace.
  Build/test with `RUSTUP_TOOLCHAIN=nightly-2026-05-28` prefixed when the shell
  pins a toolchain.
- `ui/main.rs` + `ui/main.stderr` via `dylint_testing::ui_test_examples`.

### Notes

- MSRV `1.80` for `memdet` / `memdet-loco`; `memdet-lints` is nightly-pinned.
  Dependency versions checked against crates.io on 2026-09-02.
- No CI yet; `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo test --all-features`, and `cargo test --no-default-features` are run by hand.
