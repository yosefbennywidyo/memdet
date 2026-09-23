//! A/B demonstration of the `Router::layer(Extension(..))` per-route clone.
//!
//! Run it:
//!
//! ```text
//! cargo run -p memdet --example axum_ab --features axum
//! cargo run -p memdet --example axum_ab --features axum -- buggy
//! cargo run -p memdet --example axum_ab --features axum -- fixed
//! ```
//!
//! With no argument it builds both routers (buggy first, then fixed) so the
//! summary table shows the contrast directly:
//!
//! ```text
//! memdet boot report - total +XXXX.X MiB across 2 phase(s)
//!   phase                       start          end          delta
//!   build_router_buggy   ...  +512.x MiB  <- worst  (over threshold)
//!   build_router_fixed   ...    +0.x MiB
//! ```

use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use memdet::axum::tracked_extension;
use memdet::BootReport;

/// Bytes per payload. 128 routes x 4 MiB ~= 512 MiB for the buggy variant.
const PAYLOAD_MIB: usize = 4;
const ROUTES: usize = 128;
const WARN_AT: usize = 64 * 1024 * 1024;

/// Stand-in for something expensive to clone: a view engine's parsed templates,
/// a large embedded config, a fat lookup table. `Clone` deep-copies the buffer.
#[derive(Clone)]
struct HeavyState {
    _blob: Vec<u8>,
}

impl HeavyState {
    fn new() -> Self {
        Self {
            _blob: vec![0xA5; PAYLOAD_MIB * 1024 * 1024],
        }
    }
}

fn empty_routes() -> Router {
    let mut router = Router::new();
    for i in 0..ROUTES {
        router = router.route(&format!("/r{i}"), get(|| async { "ok" }));
    }
    router
}

/// The footgun: a bare value in the layer. `Router::layer` clones it per route.
fn build_buggy(report: &BootReport) -> Router {
    let state = HeavyState::new();
    let _phase = report.bracket_warn("build_router_buggy", WARN_AT);
    empty_routes().layer(tracked_extension(state))
}

/// The fix: one `Arc`; `Router::layer` clones an 8-byte pointer per route.
fn build_fixed(report: &BootReport) -> Router {
    let state = Arc::new(HeavyState::new());
    let _phase = report.bracket_warn("build_router_fixed", WARN_AT);
    empty_routes().layer(tracked_extension(state))
}

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    let report = BootReport::new();
    let mut keep: Vec<Router> = Vec::new();

    match std::env::args().nth(1).as_deref() {
        Some("buggy") => keep.push(build_buggy(&report)),
        Some("fixed") => keep.push(build_fixed(&report)),
        _ => {
            keep.push(build_buggy(&report));
            keep.push(build_fixed(&report));
        }
    }

    println!("\n{}", report.summary_table());

    // Hold every router alive across the whole run: the buggy 512 MiB must still
    // be resident when `build_fixed` takes its *start* sample, otherwise the
    // fixed phase looks like it freed memory instead of adding almost none.
    drop(keep);
}
