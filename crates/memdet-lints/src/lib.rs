#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_hir;
extern crate rustc_middle;

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::ty::{is_copy, should_call_clone_as_function};
use rustc_hir::def::{CtorKind, CtorOf, DefKind, Res};
use rustc_hir::{Expr, ExprKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty::{self, Ty};

dylint_linting::impl_late_lint! {
    /// ### What it does
    ///
    /// Flags `axum::Extension(x)` where `x` is not behind `Arc` / `Rc` (nor a
    /// `Copy` type).
    ///
    /// ### Why is this bad?
    ///
    /// `axum::Router::layer(Extension(x))` clones `x` **once per route**, and
    /// extracting `Extension<T>` in a handler clones it **once per request**. If
    /// `x` owns heap data (a view engine, a large config, a long `Vec` /
    /// `HashMap`), each clone is a deep copy. In one real app this deep-cloned
    /// ~170 Tera template ASTs across ~190 routes into ~1.9 GB of resident memory
    /// at boot, causing a production OOM.
    ///
    /// The check is deliberately not `size_of`-based: `size_of` does not see the
    /// heap, and a `tera::Tera` handle is only a few pointers wide. It is simply
    /// "already behind `Arc` / `Rc`, or `Copy`".
    ///
    /// ### Known problems
    ///
    /// Types that are internally reference-counted (`sqlx::Pool`,
    /// `reqwest::Client`, `sea_orm::DatabaseConnection`) clone cheaply but are
    /// still flagged. List them under `allow` in `dylint.toml`:
    ///
    /// ```toml
    /// [memdet-lints]
    /// allow = ["sqlx::Pool", "sea_orm::DatabaseConnection", "reqwest::Client"]
    /// deny_heavy = ["tera::Tera", "TeraView"]
    /// ```
    ///
    /// ### Example
    ///
    /// ```ignore
    /// let engine = TeraView::build()?;
    /// router.layer(Extension(engine)); // deep clone per route
    /// ```
    ///
    /// Use instead:
    ///
    /// ```ignore
    /// router.layer(Extension(std::sync::Arc::new(engine)));
    /// ```
    pub EXTENSION_NOT_ARC,
    Warn,
    "axum `Extension` payload not wrapped in `Arc`, deep-cloned per route/request",
    ExtensionNotArc::new()
}

/// `dylint.toml` `[memdet-lints]` table.
#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct Config {
    /// Type paths (matched as substrings) that clone cheaply despite not being
    /// literally `Arc<_>` — suppressed.
    allow: Vec<String>,
    /// Type paths (substrings) known to be expensive — a firmer message.
    deny_heavy: Vec<String>,
}

struct ExtensionNotArc {
    config: Config,
}

impl ExtensionNotArc {
    fn new() -> Self {
        Self {
            config: dylint_linting::config_or_default(env!("CARGO_PKG_NAME")),
        }
    }
}

impl<'tcx> LateLintPass<'tcx> for ExtensionNotArc {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        // `Extension(<arg>)` — a call with exactly one argument.
        let ExprKind::Call(callee, [arg]) = expr.kind else {
            return;
        };
        if !is_axum_extension_ctor(cx, callee) {
            return;
        }

        let arg_ty = cx.typeck_results().expr_ty(arg).peel_refs();

        // Already cheap to clone: `Arc` / `Rc` (and weak variants), or `Copy`.
        if should_call_clone_as_function(cx, arg_ty) || is_copy(cx, arg_ty) {
            return;
        }

        let desc = ty_desc(cx, arg_ty);

        if self.config.allow.iter().any(|a| desc.contains(a.as_str())) {
            return;
        }
        let heavy = self
            .config
            .deny_heavy
            .iter()
            .any(|d| desc.contains(d.as_str()));

        let help = if heavy {
            "this type is known to be expensive to clone; `Router::layer` deep-copies it once per \
             route. Wrap it: `Extension(std::sync::Arc::new(..))`"
        } else {
            "`Router::layer` clones this once per route and `Extension<T>` extraction clones it once \
             per request. If the clone is not cheap, wrap it in `Arc`; if it is (an internally \
             ref-counted handle), add the type to `allow` in `dylint.toml`"
        };

        span_lint_and_help(
            cx,
            EXTENSION_NOT_ARC,
            arg.span,
            format!("`Extension` payload `{desc}` is not `Arc`-wrapped"),
            None,
            help,
        );
    }
}

/// True if `callee` resolves to the constructor of `axum`'s `Extension` tuple
/// struct.
fn is_axum_extension_ctor(cx: &LateContext<'_>, callee: &Expr<'_>) -> bool {
    let ExprKind::Path(qpath) = &callee.kind else {
        return false;
    };
    let Res::Def(DefKind::Ctor(CtorOf::Struct, CtorKind::Fn), ctor_did) =
        cx.qpath_res(qpath, callee.hir_id)
    else {
        return false;
    };
    let struct_did = cx.tcx.parent(ctor_did);
    cx.tcx
        .opt_item_name(struct_did)
        .is_some_and(|name| name.as_str() == "Extension")
        && cx.tcx.crate_name(struct_did.krate).as_str() == "axum"
}

fn ty_desc<'tcx>(cx: &LateContext<'tcx>, ty: Ty<'tcx>) -> String {
    if let ty::Adt(adt, _) = ty.kind() {
        cx.tcx.def_path_str(adt.did())
    } else {
        ty.to_string()
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test_examples(env!("CARGO_PKG_NAME"));
}
