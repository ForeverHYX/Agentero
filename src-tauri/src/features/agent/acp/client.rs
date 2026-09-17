use crate::core::error::AppError;
pub(crate) use crate::core::process::windows_shell_path as simplified_agent_cwd;
use crate::features::agent::acp::terminal::AcpTerminalManager;
use crate::features::agent::models::{AgentDescriptor, AgentResultPayload, AgentTemplate};

use crate::features::agent::registry::discovery::{login_shell_env, path_entries};
use agent_client_protocol::schema::v1::{
    ClientCapabilities, ElicitationCapabilities, ElicitationFormCapabilities, EnvVariable,
    InitializeRequest, McpServer, McpServerStdio,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{util, AcpAgent};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

/// Shared ACP client builder: name + terminal handler. Call sites attach their own
/// notification / request handlers.
macro_rules! agentero_acp_builder {
    ($terminals:expr) => {
        ::agent_client_protocol::Client
            .builder()
            .name("agentero")
            .with_handler(
                $crate::features::agent::acp::terminal::AcpTerminalHandler::new($terminals),
            )
    };
}
pub(crate) use agentero_acp_builder;

/// Terminal manager for one ACP connection (`Some(cwd)` → Vault cwd default).
pub(crate) fn acp_terminals(
    cwd: Option<std::path::PathBuf>,
) -> Arc<tokio::sync::Mutex<AcpTerminalManager>> {
    Arc::new(tokio::sync::Mutex::new(match cwd {
        Some(cwd) => AcpTerminalManager::with_cwd(cwd),
        None => AcpTerminalManager::new(),
    }))
}

/// Advertise form elicitation so codex-acp bridges `request_user_input` to the client,
/// and terminal execution so agents like Kimi Code can run shell commands.
pub(crate) fn client_initialize_request() -> InitializeRequest {
    InitializeRequest::new(ProtocolVersion::V1).client_capabilities(
        ClientCapabilities::new()
            .elicitation(ElicitationCapabilities::new().form(ElicitationFormCapabilities::new()))
            .terminal(true),
    )
}

/// Quote a string for a POSIX `sh -c` command so spaces/special characters are
/// preserved. Wraps in single quotes and escapes embedded single quotes.
#[cfg(not(windows))]
pub(crate) fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

/// Wrap a local agent command in a shell that changes to `cwd` before exec'ing
/// the real agent. The ACP stdio transport has no `cwd` field, so this ensures
/// agents like the Pi adapter start with the vault as their OS-level working
/// directory.
#[cfg(not(windows))]
pub(crate) fn wrap_local_command_with_cwd(
    command: &Path,
    args: &[String],
    _env: &mut HashMap<String, String>,
    cwd: &Path,
) -> (PathBuf, Vec<String>) {
    let mut script = format!(
        "cd {} && exec {}",
        shell_quote(&cwd.to_string_lossy()),
        shell_quote(&command.to_string_lossy())
    );
    for arg in args {
        script.push(' ');
        script.push_str(&shell_quote(arg));
    }
    (PathBuf::from("/bin/sh"), vec!["-c".to_string(), script])
}

/// Quote a token for a Windows `cmd /C` command. Empty strings, spaces, and
/// most cmd metacharacters trigger double-quote wrapping; internal double
/// quotes are backslash-escaped.
#[cfg(windows)]
pub(crate) fn windows_shell_quote(s: &str) -> String {
    if s.is_empty()
        || s.contains(' ')
        || s.contains('"')
        || s.contains('&')
        || s.contains('|')
        || s.contains('<')
        || s.contains('>')
        || s.contains('^')
        || s.contains('%')
    {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// Convert Rust's canonicalized local drive path into a form accepted by `cmd.exe`.
/// True UNC paths stay unchanged; supporting them requires a separate `pushd` flow.
#[cfg(any(windows, test))]
pub(crate) fn windows_cmd_cwd(cwd: &Path) -> String {
    simplified_agent_cwd(cwd).to_string_lossy().into_owned()
}

/// Pre-quote the cwd environment value so metacharacters remain literal after
/// `cmd.exe` expands `%AGENTERO_AGENT_CWD%`, even when the path has no spaces.
#[cfg(any(windows, test))]
pub(crate) fn windows_cmd_cwd_env_value(cwd: &Path) -> String {
    format!("\"{}\"", windows_cmd_cwd(cwd))
}

/// Windows variant of [`wrap_local_command_with_cwd`]. Uses `cmd /D /C` and an
/// environment variable for the cwd so spaces in the vault path do not need to
/// be quoted inside the command string.
#[cfg(windows)]
pub(crate) fn wrap_local_command_with_cwd(
    command: &Path,
    args: &[String],
    env: &mut HashMap<String, String>,
    cwd: &Path,
) -> (PathBuf, Vec<String>) {
    env.insert(
        "AGENTERO_AGENT_CWD".to_string(),
        windows_cmd_cwd_env_value(cwd),
    );
    let mut agent_command = windows_shell_quote(&command.to_string_lossy());
    for arg in args {
        agent_command.push(' ');
        agent_command.push_str(&windows_shell_quote(arg));
    }
    env.insert("AGENTERO_AGENT_COMMAND".to_string(), agent_command);
    (
        PathBuf::from("cmd"),
        vec![
            "/D".to_string(),
            "/C".to_string(),
            "cd /d %AGENTERO_AGENT_CWD% && %AGENTERO_AGENT_COMMAND%".to_string(),
        ],
    )
}

/// Summarize an ACP stdio line for debug logs without dumping the full payload.
fn summarize_acp_line(line: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
        if let Some(method) = value.get("method").and_then(|m| m.as_str()) {
            return method.to_string();
        }
        if let Some(id) = value.get("id") {
            if value.get("error").is_some() {
                return format!("error(id={id})");
            }
            return format!("response(id={id})");
        }
    }
    if line.len() > 120 {
        format!("{}...", &line[..120])
    } else {
        line.to_string()
    }
}

fn acp_agent_with_debug(agent: AcpAgent, name: &str) -> AcpAgent {
    let name = name.to_string();
    agent.with_debug(
        move |line: &str, direction: agent_client_protocol::LineDirection| {
            let summary = summarize_acp_line(line);
            log::debug!(
                target: "agentero::acp::stdio",
                "{name} {direction:?}: {summary}",
            );
            log::trace!(
                target: "agentero::acp::stdio",
                "{name} {direction:?}: {line}",
            );
        },
    )
}

fn append_path_entries(entries: &mut Vec<PathBuf>, value: Option<&String>) {
    let Some(value) = value else {
        return;
    };
    for entry in std::env::split_paths(value) {
        if !entry.as_os_str().is_empty() && !entries.iter().any(|existing| existing == &entry) {
            entries.push(entry);
        }
    }
}

/// Build the environment for an ACP child process.
///
/// Non-PATH priority (low → high): process, missing login-shell values, descriptor.
/// PATH is merged in executable-resolution order: descriptor, process, login shell,
/// then common GUI-missing locations.
pub(crate) fn build_child_env(
    process_env: impl Iterator<Item = (String, String)>,
    shell_env: Option<&HashMap<String, String>>,
    desc_env: &HashMap<String, String>,
) -> HashMap<String, String> {
    let process_env: HashMap<String, String> = process_env.collect();
    let mut child_env = process_env.clone();
    if let Some(shell_env) = shell_env {
        for (key, value) in shell_env {
            child_env.entry(key.clone()).or_insert(value.clone());
        }
    }
    for (key, value) in desc_env {
        child_env.insert(key.clone(), value.clone());
    }

    let mut merged_path = Vec::new();
    append_path_entries(&mut merged_path, desc_env.get("PATH"));
    append_path_entries(&mut merged_path, process_env.get("PATH"));
    append_path_entries(
        &mut merged_path,
        shell_env.and_then(|environment| environment.get("PATH")),
    );
    for entry in path_entries() {
        if !merged_path.iter().any(|existing| existing == &entry) {
            merged_path.push(entry);
        }
    }
    if let Ok(path) = std::env::join_paths(&merged_path) {
        child_env.insert("PATH".to_string(), path.to_string_lossy().to_string());
    }

    child_env
}

pub(crate) fn effective_local_agent_env(desc: &AgentDescriptor) -> HashMap<String, String> {
    build_child_env(std::env::vars(), login_shell_env(), &desc.env)
}

pub(crate) fn resolve_command_in_agent_env(
    command: &str,
    environment: &HashMap<String, String>,
) -> Option<PathBuf> {
    let paths = environment
        .get("PATH")
        .map(|value| std::env::split_paths(value).collect::<Vec<_>>())
        .unwrap_or_default();
    crate::core::process::resolve_command_in_paths(command, &paths)
}

pub(crate) fn to_acp_agent_local(
    desc: &AgentDescriptor,
    cwd: Option<&Path>,
) -> Result<AcpAgent, AppError> {
    let mut child_env = effective_local_agent_env(desc);
    // ZCode's adapter needs the desktop app's runtime provider table and a
    // backend CLI that still supports the provider-registry push. Skip keys
    // already present (descriptor env and any user shell export), so explicit
    // user settings always win over auto-discovery.
    if desc.template == AgentTemplate::Zcode {
        for (key, value) in crate::features::agent::registry::templates::zcode_runtime_env() {
            child_env.entry(key).or_insert(value);
        }
    }
    let command = resolve_command_in_agent_env(&desc.command, &child_env)
        .unwrap_or_else(|| PathBuf::from(&desc.command));

    let (command, args) =
        if let Some(cwd) = cwd.filter(|_| desc.template.needs_local_cwd_shell_wrap()) {
            wrap_local_command_with_cwd(&command, &desc.args, &mut child_env, cwd)
        } else {
            (command, desc.args.clone())
        };

    let env: Vec<EnvVariable> = child_env
        .into_iter()
        .map(|(k, v)| EnvVariable::new(k.clone(), v.clone()))
        .collect();

    let stdio = McpServerStdio::new(desc.name.clone(), command)
        .args(args)
        .env(env);
    Ok(acp_agent_with_debug(
        AcpAgent::new(McpServer::Stdio(stdio)),
        &desc.name,
    ))
}

/// Build ACP agent process. When `remote` is SSH, wrap launch as `ssh … 'cd vault && exec agent'`.
/// Local-sim remotes use a normal local process with cwd = remote vault path.
pub(crate) fn to_acp_agent(
    desc: &AgentDescriptor,
    cwd: Option<&Path>,
    remote: Option<&dyn crate::features::agent::remote_host::RemoteAgentLaunch>,
) -> Result<AcpAgent, AppError> {
    if let Some(r) = remote {
        if r.is_ssh() {
            let (program, args) = r.ssh_stdio(&desc.command, &desc.args, &desc.env)?;
            let stdio = McpServerStdio::new(desc.name.clone(), program).args(args);
            return Ok(acp_agent_with_debug(
                AcpAgent::new(McpServer::Stdio(stdio)),
                &desc.name,
            ));
        }
        // local-sim: local binary, cwd set via NewSessionRequest to remote_cwd
    }
    to_acp_agent_local(desc, cwd)
}

pub(crate) async fn wait_for_cancellation(cancellation: &mut watch::Receiver<bool>) {
    if *cancellation.borrow() {
        return;
    }
    let _ = cancellation.changed().await;
}

/// Shared budget for ACP session RPCs.
pub(crate) const ACP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Initialize gets a longer budget: BYOA agents bootstrap heavy runtimes
/// (python venvs, plugin and MCP discovery) before answering, and cold
/// starts routinely exceed a 15s window — two cold spawns racing after an
/// app start made Hermes miss it repeatedly. A hard 15s turns slow-but-
/// working agents into hard "agent unavailable" failures.
pub(crate) const ACP_INITIALIZE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub(crate) async fn timed_acp_request<T, E>(
    label: &str,
    request: impl std::future::Future<Output = Result<T, E>>,
) -> Result<T, agent_client_protocol::Error>
where
    E: std::fmt::Display,
{
    timed_acp_request_with(ACP_TIMEOUT, label, request).await
}

/// `initialize` variant of [`timed_acp_request`]; see [`ACP_INITIALIZE_TIMEOUT`].
pub(crate) async fn timed_acp_initialize<T, E>(
    request: impl std::future::Future<Output = Result<T, E>>,
) -> Result<T, agent_client_protocol::Error>
where
    E: std::fmt::Display,
{
    timed_acp_request_with(ACP_INITIALIZE_TIMEOUT, "initialize", request).await
}

async fn timed_acp_request_with<T, E>(
    budget: std::time::Duration,
    label: &str,
    request: impl std::future::Future<Output = Result<T, E>>,
) -> Result<T, agent_client_protocol::Error>
where
    E: std::fmt::Display,
{
    tokio::time::timeout(budget, request)
        .await
        .map_err(|_| acp_err(format!("{label} timed out after {}s", budget.as_secs())))?
        .map_err(|error| acp_err(format!("{label}: {error}")))
}

pub(crate) fn cancelled_payload(
    session_id: String,
    message_id: String,
    provider_session_id: Option<String>,
    content: &Arc<Mutex<String>>,
    thought: &Arc<Mutex<String>>,
) -> AgentResultPayload {
    let content = content
        .lock()
        .map(|buffer| buffer.clone())
        .unwrap_or_default();
    let reasoning = thought
        .lock()
        .map(|buffer| buffer.clone())
        .unwrap_or_default();
    AgentResultPayload {
        session_id,
        message_id,
        sources: Vec::new(),
        content,
        reasoning: (!reasoning.is_empty()).then_some(reasoning),
        stop_reason: Some("cancelled".to_string()),
        provider_session_id,
    }
}

pub(crate) fn acp_err(msg: impl ToString) -> agent_client_protocol::Error {
    util::internal_error(msg)
}

#[cfg(test)]
mod timeout_tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn initialize_has_an_independent_budget() {
        let slow_request = || async {
            tokio::time::sleep(std::time::Duration::from_secs(20)).await;
            Ok::<_, &str>(())
        };
        let (initialize, session) = tokio::join!(
            timed_acp_initialize(slow_request()),
            timed_acp_request("session/list", slow_request()),
        );
        assert!(initialize.is_ok());
        assert!(session
            .unwrap_err()
            .to_string()
            .contains("session/list timed out after 15s"));
        let error = timed_acp_initialize(std::future::pending::<Result<(), &str>>())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("initialize timed out after 30s"));
    }
}

#[cfg(test)]
mod cwd_shell_wrap_tests {
    use super::*;

    #[test]
    #[cfg(not(windows))]
    fn shell_quote_wraps_and_escapes_single_quotes() {
        assert_eq!(shell_quote("hello"), "'hello'");
        assert_eq!(shell_quote("it's ok"), "'it'\"'\"'s ok'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    #[cfg(not(windows))]
    fn wrap_unix_builds_sh_cd_exec_script() {
        let mut env = HashMap::new();
        let (cmd, args) = wrap_local_command_with_cwd(
            Path::new("/usr/bin/pi-acp"),
            &["--foo".to_string(), "bar baz".to_string()],
            &mut env,
            Path::new("/path/with spaces"),
        );
        assert_eq!(cmd, PathBuf::from("/bin/sh"));
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], "-c");
        assert!(args[1]
            .starts_with("cd '/path/with spaces' && exec '/usr/bin/pi-acp' '--foo' 'bar baz'"));
        assert!(env.is_empty());
    }

    #[test]
    #[cfg(windows)]
    fn windows_shell_quote_wraps_metacharacters() {
        assert_eq!(windows_shell_quote("plain"), "plain");
        assert_eq!(windows_shell_quote("with space"), "\"with space\"");
        assert_eq!(windows_shell_quote("a\"b"), "\"a\\\"b\"");
        assert_eq!(windows_shell_quote(""), "\"\"");
    }

    #[test]
    fn windows_cmd_cwd_env_value_normalizes_and_always_quotes() {
        assert_eq!(
            windows_cmd_cwd_env_value(Path::new(r"\\?\C:\Vault)")),
            r#""C:\Vault)""#
        );
        assert_eq!(
            windows_cmd_cwd_env_value(Path::new(r"C:\Vault")),
            r#""C:\Vault""#
        );
        assert_eq!(
            windows_cmd_cwd(Path::new(r"\\?\UNC\server\share")),
            r"\\?\UNC\server\share"
        );
    }

    #[test]
    #[cfg(windows)]
    fn wrap_windows_builds_cmd_cd_script() {
        let mut env = HashMap::new();
        let (cmd, args) = wrap_local_command_with_cwd(
            Path::new(r"C:\Program Files\pi-acp.cmd"),
            &["--foo".to_string(), "bar baz".to_string()],
            &mut env,
            Path::new(r"\\?\C:\My Vault"),
        );
        assert_eq!(cmd, PathBuf::from("cmd"));
        assert_eq!(
            args,
            vec![
                "/D".to_string(),
                "/C".to_string(),
                "cd /d %AGENTERO_AGENT_CWD% && %AGENTERO_AGENT_COMMAND%".to_string(),
            ]
        );
        assert_eq!(
            env.get("AGENTERO_AGENT_CWD"),
            Some(&r#""C:\My Vault""#.to_string())
        );
        assert_eq!(
            env.get("AGENTERO_AGENT_COMMAND"),
            Some(&r#""C:\Program Files\pi-acp.cmd" --foo "bar baz""#.to_string())
        );
    }

    #[test]
    fn build_child_env_priority_order() {
        let process_env = vec![
            ("OPENAI_API_KEY".to_string(), "process-key".to_string()),
            ("SHARED".to_string(), "process".to_string()),
        ]
        .into_iter();
        let mut shell_env = HashMap::new();
        shell_env.insert(
            "OPENAI_BASE_URL".to_string(),
            "https://shell/v1".to_string(),
        );
        shell_env.insert("SHARED".to_string(), "shell".to_string());
        let mut desc_env = HashMap::new();
        desc_env.insert("OPENAI_API_KEY".to_string(), "desc-key".to_string());

        let env = build_child_env(process_env, Some(&shell_env), &desc_env);

        // desc_env wins over everything.
        assert_eq!(env.get("OPENAI_API_KEY"), Some(&"desc-key".to_string()));
        // shell_env fills missing keys but does not overwrite process env.
        assert_eq!(
            env.get("OPENAI_BASE_URL"),
            Some(&"https://shell/v1".to_string())
        );
        assert_eq!(env.get("SHARED"), Some(&"process".to_string()));
    }

    #[test]
    fn build_child_env_merges_path_in_resolution_order() {
        let process_path = std::env::join_paths(["/process/bin", "/shared/bin"]).unwrap();
        let shell_path = std::env::join_paths(["/shell/bin", "/shared/bin"]).unwrap();
        let descriptor_path = std::env::join_paths(["/descriptor/bin"]).unwrap();
        let process_env = vec![(
            "PATH".to_string(),
            process_path.to_string_lossy().into_owned(),
        )]
        .into_iter();
        let mut shell_env = HashMap::new();
        shell_env.insert(
            "PATH".to_string(),
            shell_path.to_string_lossy().into_owned(),
        );
        let mut desc_env = HashMap::new();
        desc_env.insert(
            "PATH".to_string(),
            descriptor_path.to_string_lossy().into_owned(),
        );

        let env = build_child_env(process_env, Some(&shell_env), &desc_env);
        let entries = std::env::split_paths(env.get("PATH").unwrap()).collect::<Vec<_>>();

        assert_eq!(entries[0], PathBuf::from("/descriptor/bin"));
        assert_eq!(entries[1], PathBuf::from("/process/bin"));
        assert_eq!(entries[2], PathBuf::from("/shared/bin"));
        assert_eq!(entries[3], PathBuf::from("/shell/bin"));
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.as_path() == Path::new("/shared/bin"))
                .count(),
            1
        );
    }
}
