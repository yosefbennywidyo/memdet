#![doc = include_str!("../README.md")]

#[cfg(feature = "axum")]
pub mod axum;
#[cfg(feature = "hooks")]
pub mod hooks;
mod report;
mod rss;
#[cfg(feature = "testkit")]
pub mod testkit;

pub use report::{BootReport, Phase};
pub use rss::{rss_now, try_rss_now, RssBracket};

#[cfg(any(test, feature = "test-util"))]
pub use rss::{override_rss, RssOverrideGuard};
