//! Generic web paper viewer proxy (`agentero-web://`).
//!
//! Unlike the 广场 site proxies, which serve one hardcoded origin each, this
//! serves any host on the [`allowlist`] the frontend seeds per paper, so HTML
//! papers can be embedded with a selection bridge. The proxy rebuilds each
//! response to drop `X-Frame-Options` / CSP and injects the bridge script —
//! the only way the app can see text selected inside a cross-origin frame.

pub mod allowlist;
pub mod commands;
pub mod proxy;
