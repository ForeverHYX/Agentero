# PDF 重新编译后应用内仍显示旧版本

**影响面**：已打开的本地 PDF tab / PDF 翻译分屏 / 文档弹出窗；外部 LaTeX 编译或其它工具覆盖同一路径 PDF 后，Finder/系统目录里是新文件，Agentero 内仍可能显示旧页面。

## 现象

同一个 `.pdf` 文件被重新编译覆盖后，系统目录或 Preview 打开能看到最新版，但 Agentero 中已经打开的 PDF pane 仍停留在旧内容。关闭再重新打开通常会恢复，因为重新打开会从磁盘重新读取字节。

## 原因

PDF viewer 不直接用磁盘路径渲染，而是在打开时把本地 PDF 读成 `ArrayBuffer` 后交给 EmbedPDF。EmbedPDF 只会在 `pdfBytes` 引用变化时重新初始化文档。

Vault watcher 触发的 `applyDiskChange` 之前只处理 Markdown / Excalidraw / 纯文本 reseed；`.pdf` 路径只刷新文件树和索引相关状态，没有重新读取 PDF 字节并写回 tab。因此已挂载 viewer 持有旧 buffer，磁盘文件已经更新但应用内内容不会自动换。

TeX compile flow 自己会在编译完成后读取输出 PDF 并更新对应 pane；但外部编译、弹出文档窗口、翻译分屏以及 watcher 先到的普通磁盘变更仍会绕过这条路径。

## 修复

- `refreshPdfTab`：对匹配路径的 `pdf` / `translation` pane 写入新的 `pdfBytes`，用新的 `ArrayBuffer` 身份触发 EmbedPDF reload；`texCompiling` pane 由编译流程接管，避免中途写入 latexmk 的半成品。
- `applyDiskChange`：当 watcher 命中已打开 PDF 时，通过 `localFileToArrayBuffer` 重新读取磁盘文件，并调用 PDF refresh sink；没有文本 owner 时不再尝试按 UTF-8 读 PDF。
- `DocWindowRoot`：文档弹出窗也传入 `refreshPdf` sink，保证 popout PDF 与主窗行为一致。
- `.tex` autosave：文本保存成功落盘后静默触发一次编译，并在编译完成后用输出 PDF 的最新字节刷新已打开 pane；连续保存时只排一个尾随编译。
- 读取编译产物失败时显示 `errors.pdfReadFailed`，避免“编译成功但 pane 回到旧内容”静默发生。

## 回归

```bash
pnpm exec vitest run test/pdf-disk-refresh.test.ts
```

覆盖：

- watcher 命中打开的 PDF 会重新读字节并刷新 pane；
- PDF 与 translation pane 同步换 buffer；
- `texCompiling` pane 不被 watcher 中途刷新；
- 无 PDF owner 或字节读取失败时不误刷新。
