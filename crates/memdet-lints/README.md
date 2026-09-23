# memdet-lints

A [Dylint](https://github.com/trailofbits/dylint) lint for the footgun `memdet`
measures: **`axum::Extension` payloads that are not `Arc`-wrapped**.

## `extension_not_arc`

Flags `axum::Extension(x)` where `x` is not behind `Arc` / `Rc` (nor `Copy`).

`axum::Router::layer(Extension(x))` clones `x` **once per route**, and extracting
`Extension<T>` in a handler clones it **once per request**. If `x` owns heap data
(a view engine, a large config, a long `Vec` / `HashMap`), each clone is a deep
copy. In the production Loco app this was extracted from, it deep-cloned ~170 Tera template ASTs across ~190
routes into ~1.9 GB of resident memory at boot, causing a production OOM.

Not `size_of`-based (a `tera::Tera` handle is only a few pointers wide, its cost
is on the heap): the check is simply "already `Arc` / `Rc`, or `Copy`".

```rust,ignore
let engine = TeraView::build()?;
router.layer(Extension(engine));                    // warn: deep clone per route
router.layer(Extension(std::sync::Arc::new(engine))); // ok
```

## Usage

This crate builds only on the pinned nightly in `rust-toolchain.toml`
(`nightly-2026-05-28`, matching `dylint` 6.0). If your shell sets
`RUSTUP_TOOLCHAIN`, override it: `RUSTUP_TOOLCHAIN=nightly-2026-05-28 cargo build`.

```sh
cargo install cargo-dylint dylint-link
```

Point a target project at it via `Cargo.toml`:

```toml
[workspace.metadata.dylint]
libraries = [
  { path = "../memdet/crates/memdet-lints" },
]
```

then `cargo dylint --all` (or `cargo dylint extension_not_arc`).

## Configuration — `dylint.toml`

Put a `dylint.toml` in the target workspace root:

```toml
[memdet-lints]
# Type paths (matched as substrings) that clone cheaply despite not being
# literally `Arc<_>` -- no warning for these.
allow = [
  "sqlx::Pool",
  "sea_orm::DatabaseConnection",
  "reqwest::Client",
]
# Type paths (substrings) known to be expensive -- a firmer message.
deny_heavy = [
  "tera::Tera",
  "TeraView",
]
```

## Tests

`RUSTUP_TOOLCHAIN=nightly-2026-05-28 cargo test` runs the `ui/main.rs` fixture
against `ui/main.stderr`.

## License

MIT OR Apache-2.0.
