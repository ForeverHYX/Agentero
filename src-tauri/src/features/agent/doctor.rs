use crate::core::error::AppError;
use crate::features::agent::acp::client::{
    effective_local_agent_env, resolve_command_in_agent_env,
};
use crate::features::agent::models::{AgentDescriptor, AgentTemplate};
use crate::features::agent::registry::{template_info, AgentRegistry};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

const HOST_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Package-manager installs (winget / brew) download and unpack, so they need a
/// far longer budget than the `--version` probes.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum NodeInstallOutcome {
    Installed,
    Failed,
    NoPackageManager,
}

/// Doctor spawns `<tool> --version` probes for every host runtime and registered
/// Agent. The app ships as a GUI subsystem binary, so a console child without
/// this flag makes Windows allocate a visible console window — one black window
/// per probed tool on every Doctor refresh.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum HostToolStatus {
    Available,
    Missing,
    Unusable,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HostToolDiagnostic {
    pub status: HostToolStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum CodexAuthStatus {
    Authenticated,
    Unauthenticated,
    NotApplicable,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CodexAuthDiagnostic {
    pub status: CodexAuthStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HostDoctorReport {
    pub node: HostToolDiagnostic,
    pub npm: HostToolDiagnostic,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm_prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NodeInstallResult {
    pub outcome: NodeInstallOutcome,
    /// Package manager used (winget / brew), when one was available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installer: Option<String>,
    /// Failure detail when the outcome is `failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Host probe re-run after the install attempt.
    pub report: HostDoctorReport,
}

struct CommandOutput {
    success: bool,
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn codex_descriptor(registry: &AgentRegistry) -> Result<AgentDescriptor, AppError> {
    if let Some(descriptor) = registry
        .snapshot()?
        .agents
        .into_iter()
        .find(|agent| agent.template == AgentTemplate::CodexAcp)
    {
        return Ok(descriptor);
    }

    let template = template_info("codex-acp")
        .ok_or_else(|| AppError::message("Codex catalog template is unavailable"))?;
    Ok(AgentDescriptor {
        id: "catalog-codex-acp".to_string(),
        name: template.name,
        template: AgentTemplate::CodexAcp,
        command: template.command,
        args: template.args,
        env: HashMap::new(),
        available: false,
        last_error: None,
        last_probe_ok: None,
        last_probe_agent_name: None,
        last_probe_error: None,
        last_probed_at: None,
    })
}

fn diagnostic_command(path: &Path, args: &[&str]) -> Command {
    let mut command = base_command(path, args);
    hide_console_window(&mut command);
    command
}

fn base_command(path: &Path, args: &[&str]) -> Command {
    #[cfg(windows)]
    {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if matches!(extension.as_str(), "cmd" | "bat") {
            let mut command = Command::new("cmd");
            command.args(["/D", "/C"]).arg(path).args(args);
            return command;
        }
        if extension == "ps1" {
            let mut command = Command::new("powershell");
            command
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(path)
                .args(args);
            return command;
        }
    }

    let mut command = Command::new(path);
    command.args(args);
    command
}

fn hide_console_window(command: &mut Command) {
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

async fn run_command(
    path: &Path,
    args: &[&str],
    environment: &HashMap<String, String>,
) -> Result<CommandOutput, String> {
    run_command_with_timeout(path, args, environment, HOST_PROBE_TIMEOUT).await
}

async fn run_command_with_timeout(
    path: &Path,
    args: &[&str],
    environment: &HashMap<String, String>,
    timeout: Duration,
) -> Result<CommandOutput, String> {
    let mut command = diagnostic_command(path, args);
    command.env_clear().envs(environment).kill_on_drop(true);
    let output = tokio::time::timeout(timeout, command.output())
        .await
        .map_err(|_| format!("timed out after {}s", timeout.as_secs()))?
        .map_err(|error| format!("failed to start: {error}"))?;
    Ok(CommandOutput {
        success: output.status.success(),
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn first_output_line(output: &CommandOutput) -> Option<String> {
    output
        .stdout
        .lines()
        .chain(output.stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(160).collect())
}

pub(crate) async fn diagnose_tool(
    command: &str,
    environment: &HashMap<String, String>,
) -> HostToolDiagnostic {
    let Some(path) = resolve_command_in_agent_env(command, environment) else {
        return HostToolDiagnostic {
            status: HostToolStatus::Missing,
            resolved_path: None,
            version: None,
            detail: None,
        };
    };
    let resolved_path = Some(path.display().to_string());
    match run_command(&path, &["--version"], environment).await {
        Ok(output) if output.success => HostToolDiagnostic {
            status: HostToolStatus::Available,
            resolved_path,
            version: first_output_line(&output),
            detail: None,
        },
        Ok(output) => HostToolDiagnostic {
            status: HostToolStatus::Unusable,
            resolved_path,
            version: None,
            detail: Some(match output.code {
                Some(code) => format!("exited with status {code}"),
                None => "terminated without an exit status".to_string(),
            }),
        },
        Err(detail) => HostToolDiagnostic {
            status: HostToolStatus::Unusable,
            resolved_path,
            version: None,
            detail: Some(detail),
        },
    }
}

fn auth_method(output: &str) -> String {
    let lower = output.to_ascii_lowercase();
    if lower.contains("chatgpt") {
        "chatgpt".to_string()
    } else if lower.contains("api key") || lower.contains("api-key") {
        "api-key".to_string()
    } else {
        "authenticated".to_string()
    }
}

fn parse_auth_output(output: &CommandOutput) -> CodexAuthDiagnostic {
    let combined = format!("{}\n{}", output.stdout, output.stderr);
    let lower = combined.to_ascii_lowercase();
    if output.success {
        return CodexAuthDiagnostic {
            status: CodexAuthStatus::Authenticated,
            method: Some(auth_method(&combined)),
            detail: None,
        };
    }
    if lower.contains("not logged in")
        || lower.contains("login required")
        || lower.contains("not authenticated")
        || lower.contains("unauthenticated")
    {
        return CodexAuthDiagnostic {
            status: CodexAuthStatus::Unauthenticated,
            method: None,
            detail: None,
        };
    }
    CodexAuthDiagnostic {
        status: CodexAuthStatus::Unknown,
        method: None,
        detail: Some(match output.code {
            Some(code) => format!("status command exited with {code}"),
            None => "status command terminated without an exit status".to_string(),
        }),
    }
}

async fn run_auth_status(
    path: &Path,
    args: &[&str],
    environment: &HashMap<String, String>,
) -> CodexAuthDiagnostic {
    match run_command(path, args, environment).await {
        Ok(output) => parse_auth_output(&output),
        Err(detail) => CodexAuthDiagnostic {
            status: CodexAuthStatus::Unknown,
            method: None,
            detail: Some(detail),
        },
    }
}

async fn diagnose_codex_auth_in_env(
    desc: &AgentDescriptor,
    environment: &HashMap<String, String>,
) -> CodexAuthDiagnostic {
    let Some(adapter) = resolve_command_in_agent_env(&desc.command, environment) else {
        return CodexAuthDiagnostic {
            status: CodexAuthStatus::NotApplicable,
            method: None,
            detail: Some("codex-acp is not installed".to_string()),
        };
    };

    let adapter_result =
        run_auth_status(adapter.as_path(), &["cli", "login", "status"], environment).await;
    if adapter_result.status != CodexAuthStatus::Unknown {
        return adapter_result;
    }

    let Some(codex) = resolve_command_in_agent_env("codex", environment) else {
        return adapter_result;
    };
    run_auth_status(codex.as_path(), &["login", "status"], environment).await
}

pub(crate) async fn diagnose_codex_auth(desc: &AgentDescriptor) -> CodexAuthDiagnostic {
    let environment = effective_local_agent_env(desc);
    diagnose_codex_auth_in_env(desc, &environment).await
}

/// Claude Code auth state via `claude auth status` (JSON with a `loggedIn` flag).
pub(crate) async fn diagnose_claude_auth(desc: &AgentDescriptor) -> CodexAuthDiagnostic {
    let environment = effective_local_agent_env(desc);
    let Some(claude) = resolve_command_in_agent_env("claude", &environment) else {
        return CodexAuthDiagnostic {
            status: CodexAuthStatus::NotApplicable,
            method: None,
            detail: Some("claude is not installed".to_string()),
        };
    };
    match run_command(claude.as_path(), &["auth", "status"], &environment).await {
        Ok(output) => parse_claude_auth_output(&output),
        Err(detail) => CodexAuthDiagnostic {
            status: CodexAuthStatus::Unknown,
            method: None,
            detail: Some(detail),
        },
    }
}

/// `claude auth status` prints JSON like `{"loggedIn": true, "authMethod": "oauth_token"}`
/// regardless of the exit code; fall back to the text heuristics when the shape differs.
fn parse_claude_auth_output(output: &CommandOutput) -> CodexAuthDiagnostic {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&output.stdout) {
        match value.get("loggedIn").and_then(serde_json::Value::as_bool) {
            Some(true) => {
                return CodexAuthDiagnostic {
                    status: CodexAuthStatus::Authenticated,
                    method: value
                        .get("authMethod")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string),
                    detail: None,
                };
            }
            Some(false) => {
                return CodexAuthDiagnostic {
                    status: CodexAuthStatus::Unauthenticated,
                    method: None,
                    detail: None,
                };
            }
            None => {}
        }
    }
    parse_auth_output(output)
}

async fn npm_prefix(
    npm: &HostToolDiagnostic,
    environment: &HashMap<String, String>,
) -> Option<String> {
    if npm.status != HostToolStatus::Available {
        return None;
    }
    let path = PathBuf::from(npm.resolved_path.as_ref()?);
    let output = run_command(&path, &["prefix", "-g"], environment)
        .await
        .ok()?;
    if !output.success {
        return None;
    }
    first_output_line(&output)
}

fn append_npm_prefix(environment: &mut HashMap<String, String>, prefix: &str) {
    let bin = if cfg!(windows) {
        PathBuf::from(prefix)
    } else {
        PathBuf::from(prefix).join("bin")
    };
    let mut paths = environment
        .get("PATH")
        .map(|value| std::env::split_paths(value).collect::<Vec<_>>())
        .unwrap_or_default();
    if !paths.iter().any(|path| path == &bin) {
        paths.push(bin);
    }
    if let Ok(value) = std::env::join_paths(paths) {
        environment.insert("PATH".to_string(), value.to_string_lossy().into_owned());
    }
}

pub async fn diagnose_host(registry: &AgentRegistry) -> Result<HostDoctorReport, AppError> {
    let descriptor = codex_descriptor(registry)?;
    let mut environment = effective_local_agent_env(&descriptor);
    let npm = diagnose_tool("npm", &environment).await;
    let npm_prefix = npm_prefix(&npm, &environment).await;
    if let Some(prefix) = npm_prefix.as_deref() {
        append_npm_prefix(&mut environment, prefix);
    }
    let node = diagnose_tool("node", &environment).await;
    Ok(HostDoctorReport {
        node,
        npm,
        npm_prefix,
    })
}

async fn diagnose_host_in_env(environment: &HashMap<String, String>) -> HostDoctorReport {
    let npm = diagnose_tool("npm", environment).await;
    let npm_prefix = npm_prefix(&npm, environment).await;
    let node = diagnose_tool("node", environment).await;
    HostDoctorReport {
        node,
        npm,
        npm_prefix,
    }
}

/// winget package id for the official Node.js LTS installer.
#[cfg(windows)]
const NODE_WINGET_PACKAGE: &str = "OpenJS.NodeJS.LTS";

/// The system package manager that can install Node.js on this host.
fn node_installer() -> Option<(&'static str, Vec<String>)> {
    #[cfg(windows)]
    {
        if which::which("winget").is_ok() {
            return Some((
                "winget",
                vec![
                    "install".to_string(),
                    NODE_WINGET_PACKAGE.to_string(),
                    "--silent".to_string(),
                    "--accept-package-agreements".to_string(),
                    "--accept-source-agreements".to_string(),
                ],
            ));
        }
    }
    #[cfg(not(windows))]
    {
        if which::which("brew").is_ok() {
            return Some(("brew", vec!["install".to_string(), "node".to_string()]));
        }
    }
    None
}

/// A GUI app inherits PATH from its launcher, so a fresh winget install is
/// invisible to this process. Merge the machine + user PATH from the registry
/// so the re-probe can find the new node.
#[cfg(windows)]
fn refresh_path_from_registry(environment: &mut HashMap<String, String>) {
    let Ok(powershell) = which::which("powershell").or_else(|_| which::which("pwsh")) else {
        return;
    };
    let script = "$m=[Environment]::GetEnvironmentVariable('Path','Machine');$u=[Environment]::GetEnvironmentVariable('Path','User');@($m,$u) -join ';'";
    let Ok(output) = std::process::Command::new(powershell)
        .args(["-NoProfile", "-Command", script])
        .output()
    else {
        return;
    };
    let merged = String::from_utf8_lossy(&output.stdout);
    let mut paths = environment
        .get("PATH")
        .map(|value| std::env::split_paths(value).collect::<Vec<_>>())
        .unwrap_or_default();
    for entry in std::env::split_paths(merged.as_ref()) {
        if !entry.as_os_str().is_empty() && !paths.contains(&entry) {
            paths.push(entry);
        }
    }
    if let Ok(value) = std::env::join_paths(paths) {
        environment.insert("PATH".to_string(), value.to_string_lossy().into_owned());
    }
}

#[cfg(not(windows))]
fn refresh_path_from_registry(_environment: &mut HashMap<String, String>) {}

/// One-click install of Node.js through the host's package manager, followed by
/// a fresh host probe. Linux is intentionally excluded: it has no single
/// package manager and installs need sudo, so the UI falls back to manual
/// guidance there.
pub async fn install_node(registry: &AgentRegistry) -> Result<NodeInstallResult, AppError> {
    let descriptor = codex_descriptor(registry)?;
    let mut environment = effective_local_agent_env(&descriptor);

    let Some((installer, args)) = node_installer() else {
        let report = diagnose_host_in_env(&environment).await;
        return Ok(NodeInstallResult {
            outcome: NodeInstallOutcome::NoPackageManager,
            installer: None,
            error: None,
            report,
        });
    };

    let path = which::which(installer).map_err(|error| {
        AppError::message(format!(
            "resolved {installer} but failed to start it: {error}"
        ))
    })?;
    let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    let install_error =
        match run_command_with_timeout(&path, &arg_refs, &environment, INSTALL_TIMEOUT).await {
            Ok(output) if output.success => None,
            Ok(output) => Some(format!(
                "{} install failed: {}",
                installer,
                first_output_line(&output).unwrap_or_else(|| "no output".to_string())
            )),
            Err(detail) => Some(format!("{installer} install failed: {detail}")),
        };
    let outcome = match install_error {
        Some(_) => NodeInstallOutcome::Failed,
        None => NodeInstallOutcome::Installed,
    };

    refresh_path_from_registry(&mut environment);

    let report = diagnose_host_in_env(&environment).await;
    let report_ok = report.node.status == HostToolStatus::Available;
    let error = match install_error {
        Some(detail) => Some(detail),
        None if !report_ok => Some("node still unavailable after install".to_string()),
        None => None,
    };
    Ok(NodeInstallResult {
        outcome,
        installer: Some(installer.to_string()),
        error,
        report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(success: bool, code: i32, stdout: &str, stderr: &str) -> CommandOutput {
        CommandOutput {
            success,
            code: Some(code),
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        }
    }

    #[test]
    #[cfg(windows)]
    fn diagnostic_command_routes_scripts_through_shells() {
        // `hide_console_window` has no std getter, so guard the routing that the
        // console-hiding wrapper sits on top of.
        let cmd = diagnostic_command(Path::new("probe.cmd"), &["--version"]);
        assert_eq!(cmd.as_std().get_program(), "cmd");

        let ps1 = diagnostic_command(Path::new("probe.ps1"), &["--version"]);
        assert_eq!(ps1.as_std().get_program(), "powershell");

        let exe = diagnostic_command(Path::new("hermes.exe"), &["--version"]);
        assert_eq!(exe.as_std().get_program(), "hermes.exe");
    }

    #[test]
    fn parses_authenticated_chatgpt_from_stderr() {
        let result = parse_auth_output(&output(true, 0, "", "Logged in using ChatGPT\n"));
        assert_eq!(result.status, CodexAuthStatus::Authenticated);
        assert_eq!(result.method.as_deref(), Some("chatgpt"));
        assert!(result.detail.is_none());
    }

    #[test]
    fn parses_logged_out_without_returning_raw_output() {
        let result = parse_auth_output(&output(
            false,
            1,
            "",
            "Not logged in: private@example.com\n",
        ));
        assert_eq!(result.status, CodexAuthStatus::Unauthenticated);
        assert!(result.method.is_none());
        assert!(result.detail.is_none());
    }

    #[test]
    fn leaves_unrecognized_auth_failure_unknown() {
        let result = parse_auth_output(&output(false, 7, "", "unexpected failure\n"));
        assert_eq!(result.status, CodexAuthStatus::Unknown);
        assert_eq!(
            result.detail.as_deref(),
            Some("status command exited with 7")
        );
    }

    #[test]
    fn parses_claude_logged_in_json() {
        let result = parse_claude_auth_output(&output(
            true,
            0,
            "{\"loggedIn\": true, \"authMethod\": \"oauth_token\"}\n",
            "",
        ));
        assert_eq!(result.status, CodexAuthStatus::Authenticated);
        assert_eq!(result.method.as_deref(), Some("oauth_token"));
        assert!(result.detail.is_none());
    }

    #[test]
    fn parses_claude_logged_out_json_even_on_failure_exit() {
        let result = parse_claude_auth_output(&output(false, 1, "{\"loggedIn\": false}\n", ""));
        assert_eq!(result.status, CodexAuthStatus::Unauthenticated);
        assert!(result.method.is_none());
        assert!(result.detail.is_none());
    }

    #[test]
    fn falls_back_to_text_auth_parsing_for_unexpected_claude_output() {
        let result = parse_claude_auth_output(&output(
            false,
            1,
            "",
            "Not logged in: private@example.com\n",
        ));
        assert_eq!(result.status, CodexAuthStatus::Unauthenticated);
    }
}
