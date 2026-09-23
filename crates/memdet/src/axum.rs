//! Optional axum helpers. Enabled by the `axum` feature.

use std::any::type_name;
use std::panic::Location;

use axum::Extension;

/// Wrap `value` in an [`axum::Extension`], leaving a breadcrumb about what is
/// going into the layer stack.
///
/// `axum::Router::layer(Extension(value))` clones `value` **once per route**. If
/// `value` is not behind an `Arc` / `Rc` (or a small `Copy` type), that clone is
/// a deep copy per route and shows up as a boot-time RSS spike (see the crate
/// docs). This function does not prevent that. With the `tracing` feature it
/// emits, from the call site:
///
/// * `debug` when `T` looks `Arc`-shaped (a cheap per-route clone), or
/// * `warn` otherwise.
///
/// The `Arc`-shape check is a best-effort string match on [`type_name`], which
/// has no stability guarantee; treat a false negative as "look closer", not as
/// proof.
#[track_caller]
pub fn tracked_extension<T>(value: T) -> Extension<T> {
    let ty = type_name::<T>();
    let arc_shaped = is_arc_shaped(ty);
    let caller = Location::caller();

    #[cfg(feature = "tracing")]
    if arc_shaped {
        tracing::debug!(
            ty,
            at = %caller,
            "memdet: Extension payload is Arc-shaped (cheap per-route clone)"
        );
    } else {
        tracing::warn!(
            ty,
            at = %caller,
            "memdet: Extension payload is not Arc-shaped; Router::layer deep-clones it once per route"
        );
    }

    #[cfg(not(feature = "tracing"))]
    let _ = (arc_shaped, caller);

    Extension(value)
}

/// Best-effort: `type_name` renders these as `alloc::sync::Arc<..>` etc.
fn is_arc_shaped(ty: &str) -> bool {
    let head = ty.trim_start_matches('&').trim_start();
    head.starts_with("alloc::sync::Arc<") || head.starts_with("alloc::rc::Rc<")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::rc::Rc;
    use std::sync::Arc;

    #[test]
    fn detects_arc_and_rc() {
        assert!(is_arc_shaped(type_name::<Arc<String>>()));
        assert!(is_arc_shaped(type_name::<Rc<[u8]>>()));
        assert!(is_arc_shaped(type_name::<&Arc<String>>()));
    }

    #[test]
    fn flags_bare_values() {
        assert!(!is_arc_shaped(type_name::<String>()));
        assert!(!is_arc_shaped(type_name::<Vec<u8>>()));
        assert!(!is_arc_shaped(type_name::<Box<String>>()));
    }

    #[test]
    fn returns_the_extension_unchanged() {
        let Extension(value) = tracked_extension(42_u32);
        assert_eq!(value, 42);
    }
}
