# memdet-loco

Loco adaptor for [`memdet`](https://docs.rs/memdet): attribute boot-time RSS
growth to the initializer / route-build phase, and assert on it.

Loco's `Hooks` / `Initializer` traits can't be wrapped transparently, so this
crate gives you:

- **`MemdetInitializer`** — a `loco_rs::app::Initializer` you list **last** in
  `Hooks::initializers()`. Its `before_run` records a baseline; its `after_routes`
  runs after every other initializer, records the delta as one phase, and logs
  the `memdet` summary table.
- Re-exports of `memdet::hooks::{report, phase, phase_warn, boot_summary,
  log_boot_summary}` and `assert_hook_rss_delta!` for finer, per-hook attribution:
  drop `let _p = memdet_loco::phase("after_context");` into any `Hooks` method.

## Example

```rust,ignore
use loco_rs::prelude::*;
use memdet_loco::MemdetInitializer;

pub struct App;

#[async_trait]
impl Hooks for App {
    // ...

    async fn initializers(_ctx: &AppContext) -> Result<Vec<Box<dyn Initializer>>> {
        Ok(vec![
            Box::new(view_engine::ViewEngineInitializer),
            Box::new(MemdetInitializer::new()), // keep last
        ])
    }

    async fn after_context(ctx: AppContext) -> Result<AppContext> {
        let _phase = memdet_loco::phase("after_context");
        // ... connect extra pools ...
        Ok(ctx)
    }
}
```

In a boot test:

```rust,ignore
memdet_loco::assert_hook_rss_delta!(memdet_loco::report(), "initializers+routes", 128);
```

## Notes

- Under `StartMode::WorkerOnly` Loco skips `setup_routes`; the initializer's
  `after_routes` is not invoked, so this crate adds nothing there.
- Targets `loco-rs` **1.x** with `default-features = false`, so it won't force
  loco's DB/queue backends on for an app that opted out. Apps still on loco 0.16
  are not supported.
- Not a workspace default member — it pulls the `loco-rs` dependency tree. Build it
  with `cargo build -p memdet-loco`.

## License

MIT OR Apache-2.0.
