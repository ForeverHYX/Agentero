# macOS 首次精读时弹出 Music / iCloud / Desktop / Downloads 权限

**状态**：已修复（本地 Agent 进程 cwd 不再回落到 `/`）  
**Issue**：#570  
**影响面**：macOS 上所有本地 ACP Agent 的启动阶段；首次使用最明显  
**相关代码**：

- `src-tauri/src/features/agent/acp/client.rs` — `to_acp_agent_local`（本地 spawn 一律 shell `cd` 到 Vault）
- `crates/agentero-core/src/paths.rs` — `agent_scratch_dir`
- `src-tauri/src/features/agent/session/{run,warm}.rs`、`service.rs`、`acp/probe.rs` — cwd 兜底

## 1. 问题现象

macOS 上首次让 Agent 精读一篇文献并生成笔记时，系统连续弹出多条 TCC 权限请求：

- 「访问 Apple Music、你的音乐和视频活动及媒体资料库」
- 「访问 iCloud 云盘中的文件」
- 「访问下载文件夹 / 桌面文件夹」
- 「访问其他 App 的数据」

一次给一个，用户点完一个又来一个。用户会误以为 Agentero 索取与功能无关的权限。

## 2. 根因

macOS 按「负责进程（responsible process）」归属 TCC 弹窗。Agentero 从
LaunchServices（Finder / Dock / Quick Action）启动时，**进程当前工作目录是 `/`**。
ACP stdio 传输没有 cwd 字段：`NewSessionRequest.cwd` 只告诉 Agent 会话目录，
子进程的 OS-level cwd 仍继承父进程，于是 Agent 以 `/` 作为工作目录启动。

部分 Agent CLI（Codex / Grok / Pi 等）在启动阶段会以「进程 cwd」为工作区做扫描/索引。
从 `/` 递归会进入 `/Users/<user>`，依次触达 `Music`、`Desktop`、`Downloads`、
`Library/Application Support/<其它 App>`、`Library/CloudStorage`（iCloud / OneDrive），
于是对应 TCC 服务被逐个请求——`kTCCServiceMediaLibrary`、
`kTCCServiceSystemPolicyDesktopFolder`、`kTCCServiceFileProviderDomain`、
`kTCCServiceSystemPolicyAppData` 等。

早期 `needs_local_cwd_shell_wrap()` 只对 `Pi` / `Custom` 生效（见
[pi-acp-vault-cwd.md](pi-acp-vault-cwd.md)），其余模板即使会话 cwd 正确，进程 cwd 仍是 `/`。

## 3. 解决方案

1. **本地 Agent 进程一律切到 Vault**：`to_acp_agent_local` 在已知 cwd 时对所有模板做
   shell `cd` 包装（Unix `/bin/sh -c "cd … && exec …"`，Windows `cmd /D /C` + 环境变量），
   不再只限 Pi / 自定义。Agent 启动扫描因此被限制在 Vault 内。
2. **无 Vault 上下文时用私有目录兜底**：`run` / `warm` / `list` / `load` / 探针在 Vault
   路径缺失或无效时取 `agent_scratch_dir()`（`…/agentero/agent-cwd`，按需创建），
   **绝不**回落到进程 cwd。
3. 移除 `AgentTemplate::needs_local_cwd_shell_wrap`（不再有选择分支）。

## 4. 验收建议

1. macOS 上从一个非 home 目录（例如从 Finder 双击 `.app`）启动 Agentero，打开 Vault。
2. 让 Agent 精读一篇文献并生成笔记。
3. 期望：不再出现 Music / iCloud / Desktop / Downloads 的无关权限弹窗；访问 Vault
   所在卷（如 iCloud / OneDrive）本身仍可能按需请求，属正常。
4. 检查 `~/Library/Application Support/com.apple.TCC/TCC.db` 的 `access` 表，
   确认 Agentero 不再新增 `kTCCServiceMediaLibrary` / `SystemPolicyDesktopFolder` 等条目。

## 5. 边界

- 该包装只作用于本地 Agent；SSH 远程 Agent 由 `remote_agent_shell_command` 处理。
- Vault 本身位于云盘（iCloud / OneDrive / CloudStorage）时，读写 Vault 触发
  `kTCCServiceFileProviderDomain` 是预期行为，不在此修复范围。
- `dsh` 模板外层先 `cd` 到 Vault，其内部再 `cd` 到 launcher 目录，内层覆盖外层。
