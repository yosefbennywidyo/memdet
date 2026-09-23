# memdet

Boot-time memory detector for axum/tower apps (and Loco).

`memdet` measures process RSS (resident set size) around each phase of
application startup and reports which phase grew it, and by how much.

It was built for one common axum footgun. `Router::layer(Extension(value))` clones
`value` **once per route**. If `value` owns heap data (a template engine, a large
config, a long `Vec` or `HashMap`), each of those clones is a deep copy. A router
with a few hundred routes can then hold gigabytes for the life of the process, and
you usually find out from an out-of-memory kill in production. With `memdet` the
same bug is one line in a boot report, and a test can fail on it.

> **Status:** `0.1.0-alpha`. The API may still change before `0.1.0`.

## Example

```rust
use memdet::BootReport;

let report = BootReport::new();
{
    let _phase = report.bracket("build_router");
    // ... construct the router, attach layers ...
}
{
    // Also emit a `warn` event if this phase grows RSS by more than 64 MiB.
    let _phase = report.bracket_warn("connect_pools", 64 * 1024 * 1024);
    // ... open DB / cache pools ...
}
println!("{}", report.summary_table());
```

Each bracket reads RSS when it is created and again when it drops. The summary
lists phases by growth, largest first, and flags the worst one. This is the output of
the bundled A/B example (`cargo run --example axum_ab --features axum`), which
builds the same 128-route router twice: once with a bare 4 MiB `Extension` payload,
and once with the payload wrapped in `Arc`:

```text
memdet boot report - total +809.6 MiB across 2 phase(s)
  phase                           start          end          delta
  build_router_buggy            6.0 MiB    815.6 MiB     +809.6 MiB  <- worst (over threshold)
  build_router_fixed          813.8 MiB    802.6 MiB       +0.0 MiB
```

## Features

| Feature | Default | Adds |
|---|---|---|
| `tracing` | yes | A `debug` event per phase, and a `warn` event when a phase goes over its `bracket_warn` threshold. |
| `hooks` | yes | [`memdet::hooks`](https://docs.rs/memdet/latest/memdet/hooks/): a process-global report, `phase()` / `phase_warn()`, `log_boot_summary()`, and `assert_hook_rss_delta!`. |
| `axum` | no | [`memdet::axum::tracked_extension`](https://docs.rs/memdet/latest/memdet/axum/fn.tracked_extension.html): a drop-in for `Extension(value)` that logs a warning when `value` is not `Arc`-wrapped. |
| `testkit` | no | [`memdet::testkit`](https://docs.rs/memdet/latest/memdet/testkit/): boot-memory regression assertions. Pulls in [`dhat`](https://docs.rs/dhat). |
| `test-util` | no | `override_rss()`, which feeds fixed RSS values to `memdet` in downstream tests. For tests only. |

### Phases spread across functions (`hooks`)

When a phase starts in one callback and the summary is printed from another,
use the process-global report instead of passing a `BootReport` around:

```rust,ignore
fn after_context() {
    let _phase = memdet::hooks::phase("after_context");
    // ...
}

// once boot is finished:
memdet::hooks::log_boot_summary();
```

For Loco apps, [`memdet-loco`](https://docs.rs/memdet-loco) wraps this in an
`Initializer`, so all you add is one line.

### Failing a test on a regression (`testkit`)

```rust,ignore
use memdet::testkit::{measure_rss, with_dhat};
use memdet::{assert_boot_heap, assert_boot_rss};

// Deterministic: the heap high-water mark, via dhat.
let (_app, heap) = with_dhat(build_app);
assert_boot_heap!(heap.max_bytes, 256);

// Process RSS, sampled every 2 ms. Assert on a percentile, never the max.
let (_app, rss) = measure_rss(build_app);
assert_boot_rss!(rss.p95(), 600);
```

`with_dhat` needs `dhat::Alloc` installed as the test binary's global allocator,
and only one dhat profiler can run at a time, so serialise these tests. The
[`testkit` docs](https://docs.rs/memdet/latest/memdet/testkit/) cover both
constraints.

## What RSS can and cannot tell you

- RSS is **whole-process**. A bracket also counts memory that other threads
  allocate while it is open. Take measurements during single-threaded boot, or
  treat the numbers as indicative only.
- A phase that frees memory records `0`, not a negative delta.
- Allocators keep freed pages, so RSS stays high after a spike (see
  `build_router_fixed` above: it starts where the buggy phase ended). Read the
  delta, not the absolute value.
- On platforms where RSS can't be read, `rss_now()` returns `0` and
  `try_rss_now()` returns `None`.

## Related crates

- [`memdet-loco`](https://docs.rs/memdet-loco): records the Loco initializer and
  route-building phase automatically.
- `memdet-lints`: a [Dylint](https://github.com/trailofbits/dylint) lint that
  flags a non-`Arc` `axum::Extension(x)` at compile time. Not published yet.

## License

Licensed under either of Apache License 2.0 or MIT, at your option.
