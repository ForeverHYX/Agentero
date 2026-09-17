# Pi ACP 启动时未使用 Vault 工作目录

**状态**：已修复（最初只包装 Pi；见 #570 后改为**所有本地 Agent** 都注入 OS-level cwd）  
**Issue**：#441（#570 扩展）  
**影响面**：使用 Pi Agent 时的工作目录、论文/文件查找；以及所有 Agent 的启动 cwd  
**相关代码**：

- `src-tauri/src/features/agent/acp/client.rs` — `to_acp_agent_local`、`wrap_local_command_with_cwd`
- `crates/agentero-core/src/paths.rs` — `agent_scratch_dir`（无 Vault 时的安全 cwd）

## 1. 问题现象

在 Windows 上配置 Pi Agent（`pi-acp` 社区适配器）后，每次启动 Agent 提问，Pi 都会去 `C:\` 或系统默认目录查找论文/项目文件，而不是当前 Vault 目录。用户观察到 Pi 似乎“没有注入工作路径”。

## 2. 根因

ACP 的 `NewSessionRequest` 携带 `cwd` 字段，用于告知 Agent 当前会话所属项目目录。但是：

1. `agent-client-protocol` 的 `McpServerStdio` 没有 `cwd` 字段，无法直接设置子进程的 OS-level 工作目录。
2. Pi 本身没有原生 ACP 模式，`pi-acp` 适配器负责把 ACP 消息转给 `pi --mode rpc`。该适配器可能没有把 ACP 的 `cwd` 同步成 `pi` 子进程的工作目录。
3. 因此 `pi` 启动后继承的是父进程（Tauri 应用）的 cwd，在 Windows 上往往是应用安装目录或 `C:\`，导致它去错误位置查找论文。

## 3. 解决方案

本地启动一律用一个 shell 包装命令先把 OS-level 工作目录切到 Vault，再 `exec` 真正的 Agent；不再区分适配器（最初只覆盖已知不原生处理 ACP `cwd` 的 Pi）。

### 3.1 包装范围

最初只对 `Pi` / `Custom` 做 shell `cd` 包装。但即使 Agent 正确处理 ACP `NewSessionRequest.cwd`，
它的**进程 cwd** 仍是 Agentero 的 cwd——macOS 上经 LaunchServices 启动的 GUI 进程 cwd 是 `/`，
Agent 启动阶段按进程 cwd 扫描就会遍历整个文件系统（#570，见
[macos-tcc-folder-prompts.md](macos-tcc-folder-prompts.md)）。因此现在**所有**本地模板在已知
cwd 时都包装；`needs_local_cwd_shell_wrap` 已移除。

### 3.2 Unix 包装

```text
/bin/sh -c "cd '<vault>' && exec '<command>' '<arg1>' '<arg2>' ..."
```

使用单引号包装每个 token，嵌入的单引号用 `'"'"'` 转义。

### 3.3 Windows 包装

```text
cmd /D /C "cd /d "%AGENTERO_AGENT_CWD%" && \"<command>\" \"<arg1>\" ..."
```

- 用 `AGENTERO_AGENT_CWD` 环境变量传递 Vault 路径，避免在命令字符串中直接引用带空格的 Vault 路径。
- 命令和参数按需用双引号包裹，双引号内部用 `\"` 转义。

### 3.4 调用点

所有已知 Vault cwd 的本地启动路径都传入 `cwd`（统一由 `agent_spawn_cwd()` 决定）：

- `run_once` — 用户发送 prompt 时
- `warm_agent` — 聊天面板预热
- `list_acp_sessions` — 列出可恢复会话
- `load_acp_session` — 加载历史会话

以上入口在 Vault 路径缺失/无效时用 `agent_scratch_dir()`（`…/agentero/agent-cwd`）兜底，
不再回落到进程 cwd。`probe_agent` 无 Vault 上下文，本地同样传入该 scratch 目录（远端沿用其
自身 vault 路径，#570）。

## 4. 验收建议

1. 在 Windows 上打开一个 Vault，选择 Pi Agent 发送与论文相关的提问。
2. 观察 Pi 的查找/读取路径，确认它落在当前 Vault 目录下，而不是 `C:\` 或应用安装目录。
3. 在 macOS/Linux 上重复，确认 Pi 同样以 Vault 为工作目录。
4. 其他 ACP Agent（Codex、Claude ACP、Kimi Code、Grok 等）进程 cwd 同样落在 Vault，
   启动扫描不再跑到 `$HOME`（#570）。

## 5. 边界

- 该包装只作用于本地 Agent；SSH 远程 Agent 已在 `remote_agent_shell_command` 中通过 `cd` 处理工作目录。
- `dsh` 模板自己管理 launcher 目录：外层 Vault 包装后再由其自身 `cd` 进 launcher，内层覆盖外层，行为不变。
- 若 `pi-acp` 未来原生支持 ACP `cwd`，也只影响会话目录；进程 cwd 的 shell 包装对所有本地模板保留（#570）。
