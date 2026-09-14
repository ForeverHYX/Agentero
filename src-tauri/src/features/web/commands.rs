//! `web_proxy_allow_host` — seed the `agentero-web` allowlist.

use crate::core::error::ApiResult;
use serde::Deserialize;

#[derive(Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WebProxyAllowHostArgs {
    /// Public DNS host the viewer is about to load through the proxy.
    pub host: String,
}

/// Ensure `host` may be served by the `agentero-web` proxy before its viewer
/// frame loads.
///
/// Idempotent. Non-public hosts (IP literals, intranet names) are refused, so
/// the entry point itself cannot aim the proxy at link-local endpoints; the
/// caller learns that through `false` and can surface it before the frame
/// fails to load.
#[tauri::command]
#[specta::specta]
pub async fn web_proxy_allow_host(args: WebProxyAllowHostArgs) -> Result<ApiResult<bool>, String> {
    let host = super::allowlist::normalize_host(&args.host);
    let accepted = super::allowlist::is_public_dns_host(&host);
    if accepted {
        super::allowlist::allow_host(&host);
    } else {
        log::warn!(target: "agentero::web_proxy", "refused to allow non-public host: {host}");
    }
    Ok(ApiResult::ok(accepted))
}
