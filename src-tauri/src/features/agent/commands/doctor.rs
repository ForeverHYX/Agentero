#[cfg(not(target_os = "ios"))]
use crate::app::terminal::open_terminal_confirm_login;
use crate::core::error::{map_err, ApiResult};
use crate::features::agent::doctor::{
    diagnose_host, install_node, HostDoctorReport, NodeInstallResult,
};
use crate::features::agent::doctor_agents::{diagnose_agents, AgentAcpDiagnostic};
use crate::features::agent::registry::template_info;
use crate::features::agent::{AgentRegistry, AgentWarmGate};
use tauri::{AppHandle, State};

#[tauri::command]
#[specta::specta]
pub async fn doctor_check_host(
    registry: State<'_, AgentRegistry>,
) -> Result<ApiResult<HostDoctorReport>, String> {
    Ok(match diagnose_host(registry.inner()).await {
        Ok(report) => ApiResult::ok(report),
        Err(error) => map_err(error),
    })
}

/// One-click install of Node.js via the host package manager (winget / brew),
/// then re-probe. Can take several minutes while the installer downloads.
#[tauri::command]
#[specta::specta]
pub async fn doctor_install_node(
    registry: State<'_, AgentRegistry>,
) -> Result<ApiResult<NodeInstallResult>, String> {
    Ok(match install_node(registry.inner()).await {
        Ok(result) => ApiResult::ok(result),
        Err(error) => map_err(error),
    })
}

/// Re-probe every registered Agent over ACP and return classified failures.
/// Can take up to ~30s per slow agent (probes run with limited concurrency).
#[tauri::command]
#[specta::specta]
pub async fn doctor_check_agents(
    app: AppHandle,
    registry: State<'_, AgentRegistry>,
    warm_gate: State<'_, AgentWarmGate>,
) -> Result<ApiResult<Vec<AgentAcpDiagnostic>>, String> {
    Ok(
        match diagnose_agents(registry.inner(), warm_gate.inner(), &app).await {
            Ok(report) => ApiResult::ok(report),
            Err(error) => map_err(error),
        },
    )
}

/// Open the template-owned CLI login command in a confirm-to-run terminal.
#[cfg(not(target_os = "ios"))]
#[tauri::command]
#[specta::specta]
pub fn doctor_open_agent_login_terminal(template_id: String) -> ApiResult<()> {
    let Some(info) = template_info(&template_id) else {
        return map_err(crate::core::error::AppError::message(
            "unknown agent template",
        ));
    };
    let Some(command) = info
        .login_command
        .as_deref()
        .map(str::trim)
        .filter(|command| !command.is_empty())
    else {
        return map_err(crate::core::error::AppError::message(
            "agent template does not define a login command",
        ));
    };
    match open_terminal_confirm_login(command) {
        Ok(()) => ApiResult::ok(()),
        Err(error) => map_err(error),
    }
}
