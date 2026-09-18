//! Tauri command for resolving agent inline citation links.

use crate::core::error::{map_err, ApiResult, AppError};
use crate::features::agent::citation::{resolve_citation, CitationTarget};
use crate::features::agent::remote_host::RemoteAgentHosts;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

/// Resolve a vault-relative citation link to PDF coordinates.
///
/// Supports fragments like `#section=2.3`, `#figure=1`, `#table=2`,
/// `#algorithm=1`, `#formula=5`, `#region=figure-1`, and `#page=3`.
/// Remote vaults are not supported yet.
#[tauri::command]
#[specta::specta]
pub async fn agent_resolve_citation(
    remote_registry: State<'_, Arc<dyn RemoteAgentHosts>>,
    vault_path: Option<String>,
    source: String,
) -> Result<ApiResult<CitationTarget>, String> {
    let remote = match remote_registry.resolve_target(vault_path.as_deref()).await {
        Ok(t) => t,
        Err(e) => return Ok(map_err(e)),
    };
    if remote.is_some() {
        return Ok(map_err(AppError::domain(
            "citation_remote_unsupported",
            "citation resolution for remote vaults is not supported yet",
        )));
    }
    // Citations are vault-relative; without a valid local vault there is no
    // meaningful root, so fail instead of falling back to the process cwd
    // (which is `/` for a Finder-launched macOS GUI app).
    let vault = vault_path
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .ok_or_else(|| {
            AppError::domain(
                "citation_vault_unavailable",
                "a valid local vault is required to resolve citations",
            )
        });
    let vault = match vault {
        Ok(vault) => vault,
        Err(e) => return Ok(map_err(e)),
    };

    match resolve_citation(&vault, &source) {
        Ok(target) => Ok(ApiResult::ok(target)),
        Err(e) => Ok(map_err(e)),
    }
}
