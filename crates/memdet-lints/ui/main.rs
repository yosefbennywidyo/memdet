use std::sync::Arc;

use axum::Extension;

#[derive(Clone)]
struct HeavyState {
    _templates: Vec<String>,
}

// --- ok: nothing to warn about ---

fn cheap_copy() -> Extension<u64> {
    Extension(42)
}

fn arc_param(state: Arc<HeavyState>) -> Extension<Arc<HeavyState>> {
    Extension(state)
}

fn arc_new() -> Extension<Arc<HeavyState>> {
    Extension(Arc::new(HeavyState { _templates: vec![] }))
}

// --- warn: bare owned payloads, deep-cloned per route/request ---

fn bare_struct(state: HeavyState) -> Extension<HeavyState> {
    Extension(state)
}

fn bare_string() -> Extension<String> {
    Extension(String::new())
}

fn bare_vec(v: Vec<u8>) -> Extension<Vec<u8>> {
    Extension(v)
}

fn main() {}
