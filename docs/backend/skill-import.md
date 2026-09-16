# 魔棒解析 GitHub Skill 入库（Skill Import）

> 状态：首版已实现。对应 [#118](https://github.com/poco-ai/Agentero/issues/118)。魔棒（⇧⌘I）除论文标识符外，还能识别 **GitHub 仓库链接 / `npx skills add` 指令**，把 Agent Skill 下载到当前 Vault 的 `.agents/skills/<name>/`。相关：[paper-import.md](paper-import.md)、[agent.md](agent.md)。

## 1. 背景与现状

- 魔棒输入的类型识别完全在 Host 侧：`crates/agentero-core/src/features/paper/scholar_api/sources/skill.rs` 的 `extract_skill_source()`（`SkillSource`，识别为 `skill` kind，不入 resolver 表，但由 `scholar_api::identifiers` 统一 re-export）和 `import::identifiers::extract_primary_identifier()`（其余标识符走 `scholar_api::identifiers::resolver` 静态表）。Skill 识别发生在普通 URL 分支之前，GitHub 链接不会再被送入 Translator `/web`。
- 前端不做识别，只切分多条输入（`src/components/sidebar/vault-sidebar-header.tsx` `parseLookupTexts()`），经 `lookup_import_batch`（`src-tauri/src/features/import/commands.rs:17`）进入 `import_by_identifier_batch`。
- Skill 目录约定已存在：`vault/.agents/skills/<id>/SKILL.md`（YAML frontmatter `name` / `description`，解析见 `src-tauri/src/features/agent/prompt/skills.rs` `parse_skill_metadata`）；`ensure_vault()` 播种 bundled skills，并按 frontmatter 整数 `version` 升级第一方 Skill（盘上版本低于模板则覆盖；无 `version` / 同版本 / 更高版本不覆盖）。
- 文件树中 `.agents` 是 eager 目录（`src/lib/vault/tree.ts` `TREE_EAGER_ROOT_NAMES` / `TREE_ALLOWED_DOT_NAMES`），写入后 `refreshTree` 即可见。
- 仓库内**没有任何 git clone / npm 逻辑**；候选发现与选中目录下载都走 GitHub REST API，不执行第三方代码。

### 社区规范（2026）

- Skill = 自包含目录：`SKILL.md`（必需，frontmatter `name` ≤64 小写字母数字连字符、**必须与目录名一致**；`description` ≤1024）+ 可选 `scripts/` `references/` `assets/`。规范见 [agentskills.io/specification](https://agentskills.io/specification)。
- 事实标准安装器为 **`npx skills`**（vercel-labs/skills）：`npx skills add <owner>/<repo> [--skill <name>]`，装到 `./.agents/skills/`（通用目录，与 Agentero 一致）；[skills.sh](https://skills.sh/) 页面复制出来的就是这条命令。
- 分发主体是 GitHub repo（单 skill repo 或 monorepo 如 `anthropics/skills`），npm 包分发非主流。

## 2. 输入形态识别（pattern 枚举）

新增 `SkillSource` 识别，插在 `extract_primary_identifier()` 的 URL 短路之前（或在 URL 分支内按 host 细分）。按优先级：

| # | 形态 | 示例 | 解析结果 |
|---|---|---|---|
| 1 | `npx skills add …` 指令 | `npx skills add vercel-labs/agent-skills --skill frontend-design` | 剥出 source + `--skill` 过滤（可多个；`'*'` = 全部） |
| 2 | GitHub repo 根 | `https://github.com/anthropics/skills`（含 `.git` 后缀） | owner/repo，默认分支 |
| 3 | GitHub tree URL | `https://github.com/openai/skills/tree/main/skills/.experimental/create-plan` | owner/repo + ref + 子目录 |
| 4 | `github:` 前缀 | `github:owner/repo` | 同 2 |
| 5 | skills.sh 页面 URL | `https://skills.sh/<owner>/<repo>/<skill>` | owner/repo + skill 过滤 |

不做（首版）：GitLab、SSH 形态（`git@…`）、裸 `owner/repo` shorthand（与论文标识符歧义太大，如 `10.1234/abc`）、`.well-known` skill 源、npm 包名。尾部 `#<ref>` fragment 支持指定分支/tag。

判定为 SkillSource 的条件从严：host 必须是 `github.com` / `skills.sh`，或整条输入以 `npx skills add` 开头；其余仍走论文 `Url` 路径，避免误伤网页论文入库。

## 3. 下载与落盘策略

**不依赖本机 git / node**，按输入形态下载：

1. GitHub repo 根：ref 缺省时先经 `api.github.com/repos/{owner}/{repo}` 拿默认分支；候选发现阶段不下载仓库 tarball，而是 `GET /repos/{owner}/{repo}/git/trees/{ref}?recursive=1` 列路径，只读取匹配的 `SKILL.md` blob 来展示名称/描述。用户确认后才下载选中的 Skill 目录内容。
   - 已走 Host 网络代理（`networkProxy*`）。
   - GitHub REST API 请求优先走本机已登录的 GitHub CLI：`gh api --hostname github.com ...`。没有 `gh`、未登录或 `gh api` 失败时，回退到 Host 自己的直连 `api.github.com` 请求（可复用 `GH_TOKEN` / `GITHUB_TOKEN` 或 `gh auth token`）；token 只发给 `api.github.com`，不会发给第三方 URL 前缀镜像。
   - Settings → 通用 → **GitHub 镜像**（`githubMirrorEnabled` / `githubMirrorBaseUrl`）：从内置预设列表中选择一个 URL 前缀镜像；直连失败（超时/连接错误/5xx/429/403）时回退 `{base}/https://api|codeload.github.com/...`（如 `https://gh.llkk.cc`）。GitHub `403` 常见于未认证 API 配额、出口 IP 或区域网络拦截，按可恢复网络问题处理；**其它 4xx（如 404）不回退**。与网络代理正交。
2. GitHub tree 子目录：先经 `api.github.com/repos/{owner}/{repo}/contents/{subpath}?ref={ref}` 递归列目录，只读取候选目录里的 `SKILL.md` blob 来展示候选；不下载 `assets/`、`references/`、`scripts/` 等 payload。这样 monorepo 中的单个 Skill 不需要下载整仓 tarball，且发现阶段很轻。
3. 解压或列目录后，递归扫描 `**/SKILL.md`：
   - 有子目录约束（tree URL / `--skill`）→ 只取匹配项；
   - repo 根或指定路径发现 Skill → 返回候选列表，前端弹选择（见 §4）。
4. 校验 frontmatter：`name` 合法且与目录名一致；不一致时以 `name` 为准命名目标目录。frontmatter 解析统一走 `crates/agentero-core/src/frontmatter.rs`（`frontmatter_block` + `scalar_field`，支持引号、`>` / `|` 折叠块、多行续行、CRLF，只读顶层键）。`description` 超过 1024 **字符**时按字符截断展示，不再拒绝安装；单个 `SKILL.md` 解析失败只跳过该候选，整个来源没有可用 Skill 才报错。见 [bug_fix/skill-import-description-length.md](../bug_fix/skill-import-description-length.md)。
5. 将 sparse 候选 metadata 暂存为一次性 discovery，并返回前端选择；此阶段不写入 Vault，也不下载非 `SKILL.md` payload。
6. 用户确认后才落盘 `vault/.agents/skills/<name>/`（整目录拷贝，含 `references/` 等）。这一步只下载被选中的 Skill 目录；文件内容走 GitHub Blob API（`api.github.com/repos/{owner}/{repo}/git/blobs/{sha}`），不依赖 `raw.githubusercontent.com`：
   - 目标已存在 → 默认**不覆盖**，报「已存在，是否更新」（沿用 `ensure_vault` 不覆盖用户文件的原则）；
   - 在 skill 目录写 `agentero-skill.json` 来源记录（source URL + ref + 安装时间），为将来「检查更新」留钩子（参考 `skills-lock.json` / Obsidian BRAT）。
7. **不落 catalog**（skill 不是论文；`Url` kind 已有 `identifier_kind_column → None` 先例），不建 `papers/` 条目、不跑 `paper_commit`。

私有 repo / 浅 clone 回退、GitLab 支持均延后。

## 4. 交互流程

- 入口：魔棒粘贴 → `lookup_import_batch`；广场 **Skill 推荐** 卡片点击同一条路（`importPlazaSkillRepo` → `lookupSubmit`）。batch 内论文与 skill 可混合，逐条按 kind 分发。
- 进度沿用左下角后台任务条（`job:progress`）。
- 当前实现先返回 `LookupImportBatchResult.skillCandidates`，前端打开多选 Dialog。`--skill <name>`、GitHub tree 子目录会缩小候选范围。
- 用户取消时删除 discovery 临时包；确认后调用 `skill_install`，再 `refreshTree` + toast 汇总安装结果。**不** `openPaper`、不进补资产队列。论文和 Skill 可混合提交。
- 远程 Vault：首版禁用（提示暂不支持），后续经 remote bridge + SFTP 镜像（`features/remote/launch.rs` 已有 skills 镜像逻辑）。

## 5. 安全

- tar 解压必须走 `sanitize_tar_path` 防路径穿越；限制解压总大小与文件数上限。
- 只下载解包，不执行任何脚本（skill 的 `scripts/` 仅落盘）；不 shell out 到 `npx` / `git`。
- SKILL.md 是注入 Agent prompt 的内容，安装第三方 skill 等于引入外部指令——安装 toast / 选择对话框中提示来源，用户自担信任决策（BYOA 一致原则）。

## 6. 已验证输入

| 输入 | 解析结果 |
|---|---|
| `https://github.com/mattpocock/skills` | GitHub 仓库 `mattpocock/skills`，默认分支 |
| `https://github.com/alchaincyf/nuwa-skill` | GitHub 仓库 `alchaincyf/nuwa-skill`，默认分支 |
| `npx skills add https://github.com/anthropics/skills --skill pptx` | GitHub 仓库 `anthropics/skills`，Skill 过滤 `pptx` |

回归测试位于 `crates/agentero-core/src/features/paper/scholar_api/sources/skill.rs` 的
`parses_requested_skill_import_examples`，`import/search_router.rs` 仍保留
`extract_primary_identifier` 的优先级测试。

## 7. 后续

| 阶段 | 内容 |
|---|---|
| P2 | 已装 Skill 的「检查更新 / 重装」（基于 `agentero-skill.json`）；`#ref` pin；多候选选择对话框 |
| P3 | 私有 repo（gh / git 回退）、GitLab、`.well-known` 源、远程 Vault |
