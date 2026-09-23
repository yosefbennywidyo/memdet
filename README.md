# memdet

**Boot-time memory detector for axum/tower apps (and Loco).** Framework-agnostic
core; thin per-framework adaptors.

Targets a common footgun: `axum::Router::layer(L)` clones `L` **once per route**.
If `L` wraps an expensive-to-clone value (a view engine, a large config, a long
`Vec`/`HashMap`, a client that isn't internally `Arc`), boot cost = size × route
count, held for the whole process lifetime, and it usually surfaces as an OOM.
(Origin: a production Loco app — a `TeraView` with ~170 template ASTs × ~190 routes ≈
2–3 GB boot RSS.)

**Status:** `0.1.0-alpha`. The API may still change before `0.1.0`.

## Three parts

| | Crate | When | What |
|---|---|---|---|
| **A — measure** | `memdet` (core) | runtime, stable | `RssBracket` + `BootReport`: process RSS per boot phase, and a summary table that points at the phase that grew. Feature `hooks` (default): a process-global report and `phase()`. Feature `axum`: `tracked_extension`. |
| **A-loco** | `memdet-loco` | runtime, stable | `MemdetInitializer`, a Loco `Initializer` you list last. It records the initializer + route-building span as one phase and logs the summary. |
| **B — lock** | `memdet` feat. `testkit` | test, stable | Wraps [`dhat-rs`](https://docs.rs/dhat): `with_dhat` + `assert_boot_heap!` (deterministic). Process-RSS side: `measure_rss` / `measure_rss_async` + `assert_boot_rss!` on a percentile. Per-phase: `assert_hook_rss_delta!`. |
| **C — prevent** | `memdet-lints` | compile-time, nightly | Dylint lint `extension_not_arc`: flags `axum::Extension(x)` when `x` is not `Arc`/`Rc`/`Copy`, with `allow` / `deny_heavy` lists in `dylint.toml`. Not published. |

C is optional. Usage for each crate is in its own README:
[`memdet`](crates/memdet/README.md), [`memdet-loco`](crates/memdet-loco/README.md),
[`memdet-lints`](crates/memdet-lints/README.md).

## Development

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --no-default-features
# memdet-loco is not a default workspace member (it pulls in loco-rs):
cargo clippy -p memdet-loco --all-targets -- -D warnings && cargo test -p memdet-loco
```

`memdet-lints` is a separate workspace pinned to its own nightly; see its README.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in this work by you, as defined in the Apache-2.0 license, shall be dual
licensed as above, without any additional terms or conditions.
