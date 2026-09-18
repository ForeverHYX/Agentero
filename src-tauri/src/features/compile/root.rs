//! LaTeX root-file resolution for multi-file projects.
//!
//! Compiling a `\input`/`\include` child directly cannot produce a PDF (it has
//! no `\documentclass`); the build target must be the project's root file.
//! Resolution mirrors LaTeX Workshop's strategy, simplified to what a local
//! vault needs and done in-process so one IPC call returns the answer:
//! 1. `% !TEX root = …` magic-comment chain (loop detection included),
//! 2. the file itself carrying `\documentclass` / `\begin{document}`,
//! 3. a vault-wide reverse scan: which root-indicator file's input closure
//!    (root → sub → … → target) contains this file,
//! 4. fallback: compile the file itself (standalone docs, snippets).
//!
//! Subfiles (`\documentclass[..]{subfiles}`) are intentionally out of scope.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use agentero_core::features::vault::tree::should_ignore;

use crate::core::blocking::run_blocking;
use crate::core::error::{ApiResult, AppError};

/// How the compile root was determined — for logs/debugging only; the compile
/// pipeline treats every variant identically.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum LatexRootSource {
    /// `% !TEX root = …` magic-comment chain (loop detection included).
    MagicComment,
    /// The file itself carries `\documentclass` / `\begin{document}`.
    SelfIndicator,
    /// Vault scan found a root whose input/include closure contains the file.
    VaultScan,
    /// Nothing found — the file compiles itself.
    FallbackSelf,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LatexRoot {
    pub root_path: String,
    pub source: LatexRootSource,
}

/// Magic-comment chains this long are treated as pathological and cut off.
const MAX_MAGIC_HOPS: usize = 32;
/// Vault-scan depth cap, aligned with the file tree's `MAX_DEPTH`.
const SCAN_MAX_DEPTH: usize = 12;
/// Collect at most this many .tex files per scan; past the cap the walk stops
/// with a warning and the already-collected files still form the graph.
const MAX_SCAN_FILES: usize = 5000;
/// Read at most this many bytes per file (a pathological file is truncated by
/// the read, not skipped — root indicators live in the preamble anyway).
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

#[tauri::command]
#[specta::specta]
pub async fn resolve_latex_root(tex_path: String, vault_path: String) -> ApiResult<LatexRoot> {
    run_blocking(move || {
        let tex = PathBuf::from(&tex_path);
        if !tex.is_file() {
            return ApiResult::err(AppError::message(format!(
                "tex file not found: {}",
                tex.display()
            )));
        }
        let vault = PathBuf::from(&vault_path);
        let vault = if vault.as_os_str().is_empty() {
            None
        } else {
            Some(vault)
        };
        let root = resolve_root(&tex, vault.as_deref());
        log::info!(
            "latex root of {}: {} ({:?})",
            tex_path,
            root.root_path,
            root.source
        );
        ApiResult::ok(root)
    })
    .await
}

fn resolve_root(tex: &Path, vault: Option<&Path>) -> LatexRoot {
    let content = read_lossy(tex).unwrap_or_default();
    if let Some(root) = find_magic_root(tex, &content) {
        return LatexRoot {
            root_path: root.to_string_lossy().into_owned(),
            source: LatexRootSource::MagicComment,
        };
    }
    if has_indicator(&strip_comments(&content)) {
        return LatexRoot {
            root_path: tex.to_string_lossy().into_owned(),
            source: LatexRootSource::SelfIndicator,
        };
    }
    // Scan from the vault when the file lives under it (path shapes come from
    // the same frontend store, so the prefix comparison holds); otherwise the
    // file's own directory — a same-folder root is still discoverable there.
    let scan_root = vault
        .filter(|v| tex.starts_with(v))
        .unwrap_or_else(|| tex.parent().unwrap_or(Path::new("")));
    if let Some(root) = scan_for_root(scan_root, tex) {
        return LatexRoot {
            root_path: root.to_string_lossy().into_owned(),
            source: LatexRootSource::VaultScan,
        };
    }
    LatexRoot {
        root_path: tex.to_string_lossy().into_owned(),
        source: LatexRootSource::FallbackSelf,
    }
}

/// Follow the `% !TEX root` chain starting at `tex` (LaTeX Workshop
/// `findFromMagic` semantics): every file on the chain declares the next as
/// its root; the last declaration wins. A declaration pointing at a file that
/// cannot be read abandons the whole strategy; a loop resolves to the file on
/// the loop (it is explicitly declared a root).
fn find_magic_root(tex: &Path, content: &str) -> Option<PathBuf> {
    let mut stack: Vec<PathBuf> = Vec::new();
    let mut current = tex.to_path_buf();
    let mut text = content.to_string();
    for _ in 0..MAX_MAGIC_HOPS {
        let Some(value) = parse_magic_comment(&text) else {
            break;
        };
        let target = normalize_lexical(&current.parent().unwrap_or(Path::new("")).join(&value));
        if stack.iter().any(|p| p == &target) {
            return Some(target);
        }
        stack.push(target.clone());
        let next = read_lossy(&target)?;
        current = target;
        text = next;
    }
    stack.last().cloned()
}

/// Extract the value of the first `% !TEX root = value` line, if any. The
/// directive is case-insensitive and the value must end in `.tex`; other
/// comment lines are ordinary comments.
fn parse_magic_comment(content: &str) -> Option<String> {
    content.lines().find_map(parse_magic_line)
}

fn parse_magic_line(line: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix('%')?.trim_start();
    let rest = rest.strip_prefix('!')?.trim_start();
    let (word, rest) = split_word(rest)?;
    if !word.eq_ignore_ascii_case("tex") {
        return None;
    }
    let (word, rest) = split_word(rest.trim_start())?;
    if !word.eq_ignore_ascii_case("root") {
        return None;
    }
    let rest = rest.trim_start().strip_prefix('=')?;
    let value = rest.trim();
    if value.is_empty() || !value.to_lowercase().ends_with(".tex") {
        return None;
    }
    Some(value.to_string())
}

fn split_word(s: &str) -> Option<(&str, &str)> {
    let end = s
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(s.len());
    if end == 0 {
        None
    } else {
        Some((&s[..end], &s[end..]))
    }
}

/// Strip `%`-comments line by line. A `%` preceded by an odd number of
/// backslashes (`\%`) is a literal percent sign, so the scan steps over
/// backslash + escaped-char pairs. Verbatim environments are NOT stripped (a
/// `\documentclass` inside verbatim would read as an indicator; the fallout —
/// compiling that file itself — is visible and recoverable, so the
/// simplification is accepted).
fn strip_comments(content: &str) -> String {
    content
        .lines()
        .map(truncate_at_comment)
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_at_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 1;
                // Skip the escaped char: `\\` is a literal backslash, `\%` a
                // literal percent — neither starts a comment.
                if i < bytes.len() {
                    i += 1;
                }
            }
            b'%' => return &line[..i],
            _ => i += 1,
        }
    }
    line
}

/// Root indicator on comment-stripped content: `\documentclass` or
/// `\begin{document}` with a word boundary after the match (so
/// `\documentclassfoo` does not count).
fn has_indicator(stripped: &str) -> bool {
    contains_word(stripped, "\\documentclass") || contains_word(stripped, "\\begin{document}")
}

fn contains_word(haystack: &str, needle: &str) -> bool {
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(needle) {
        let abs = start + pos;
        let after = haystack[abs + needle.len()..].chars().next();
        if !after.is_some_and(|c| c.is_ascii_alphabetic()) {
            return true;
        }
        start = abs + needle.len();
    }
    false
}

/// Parse `\input{…}` / `\include{…}` arguments from comment-stripped
/// content. The command name must match exactly, which excludes
/// `\includegraphics` / `\includeonly` / `\includepdf` / `\includesvg`. The
/// bare form (`\input foo`) is not parsed, matching LaTeX Workshop's regex.
fn parse_input_refs(stripped: &str) -> Vec<String> {
    let bytes = stripped.as_bytes();
    let mut refs = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            i += 1;
            continue;
        }
        let name_start = i + 1;
        let mut j = name_start;
        while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
            j += 1;
        }
        if j == name_start {
            // Lone backslash (e.g. `\\`); step past it.
            i += 1;
            continue;
        }
        let name = &stripped[name_start..j];
        i = j;
        if name != "input" && name != "include" {
            continue;
        }
        if i < bytes.len() && bytes[i] == b'*' {
            i += 1;
        }
        i = skip_ws(bytes, i);
        // Optional `[…]` argument (no nesting).
        if i < bytes.len() && bytes[i] == b'[' {
            while i < bytes.len() && bytes[i] != b']' {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
            i = skip_ws(bytes, i);
        }
        if i < bytes.len() && bytes[i] == b'{' {
            let arg_start = i + 1;
            if let Some(end) = stripped[arg_start..].find('}') {
                refs.push(stripped[arg_start..arg_start + end].trim().to_string());
                i = arg_start + end + 1;
            }
        }
    }
    refs
}

fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// Resolve an `\input`/`\include` argument relative to the declaring file's
/// directory: the argument as written, then with `.tex` appended (the bare
/// extension-less form), returning the candidate that exists on disk.
fn resolve_input(dir: &Path, arg: &str) -> Option<PathBuf> {
    let arg = arg.trim();
    if arg.is_empty() {
        return None;
    }
    let joined = if Path::new(arg).is_absolute() {
        PathBuf::from(arg)
    } else {
        dir.join(arg)
    };
    if joined.is_file() {
        return Some(joined);
    }
    let with_ext = {
        let mut s = joined.clone().into_os_string();
        s.push(".tex");
        PathBuf::from(s)
    };
    if with_ext.is_file() {
        return Some(with_ext);
    }
    None
}

/// Lexically fold `.` / `..` without touching the filesystem, so returned
/// paths keep the caller's prefix shape (never `canonicalize()` a return
/// value — on Windows it grows a `\\?\` verbatim prefix that breaks the
/// frontend's fs-scope matching).
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                _ => out.push(comp.as_os_str()),
            },
            _ => out.push(comp.as_os_str()),
        }
    }
    out
}

/// Filesystem identity for graph keys only: canonicalize resolves symlinks
/// and case (macOS) so equality holds across path shapes.
fn canonical_key(p: &Path) -> PathBuf {
    fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn read_lossy(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Reverse-scan the vault for a root file whose input/include closure
/// contains `target`. Builds a referenced→referencer index over every .tex
/// file under `scan_root`, then BFS upward from `target`; among the reachable
/// root-indicator files the (shallowest, lexicographically smallest) path
/// wins, keeping the choice deterministic when several roots include the
/// same child.
fn scan_for_root(scan_root: &Path, target: &Path) -> Option<PathBuf> {
    let mut files: Vec<PathBuf> = Vec::new();
    collect_tex_files(scan_root, 0, &mut files);

    let mut is_root: HashSet<PathBuf> = HashSet::new();
    let mut reverse: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    let mut display: HashMap<PathBuf, PathBuf> = HashMap::new();
    for file in &files {
        let key = canonical_key(file);
        display.insert(key.clone(), file.clone());
        let Some(content) = read_lossy(file) else {
            continue;
        };
        let stripped = strip_comments(&content);
        if has_indicator(&stripped) {
            is_root.insert(key.clone());
        }
        let parent = file.parent().unwrap_or(Path::new(""));
        for arg in parse_input_refs(&stripped) {
            if let Some(dep) = resolve_input(parent, &arg) {
                let dep_key = canonical_key(&dep);
                if dep_key != key {
                    reverse.entry(dep_key).or_default().push(key.clone());
                }
            }
        }
    }

    let target_key = canonical_key(target);
    if !display.contains_key(&target_key) {
        return None;
    }
    let mut best: Option<(usize, &PathBuf)> = None;
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
    visited.insert(target_key.clone());
    queue.push_back((target_key, 0));
    while let Some((node, depth)) = queue.pop_front() {
        if let Some(parents) = reverse.get(&node) {
            for parent in parents {
                if is_root.contains(parent) {
                    let better = match &best {
                        None => true,
                        Some((b_depth, b_path)) => {
                            (depth, parent.as_path()) < (*b_depth, b_path.as_path())
                        }
                    };
                    if better {
                        best = Some((depth, parent));
                    }
                }
                if visited.insert(parent.clone()) {
                    queue.push_back((parent.clone(), depth + 1));
                }
            }
        }
    }
    best.map(|(_, key)| display.get(key).cloned().unwrap_or_else(|| key.clone()))
}

fn collect_tex_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > SCAN_MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_SCAN_FILES {
            log::warn!(
                "latex root scan hit the {}-file cap under {}; continuing with the files collected so far",
                MAX_SCAN_FILES,
                dir.display()
            );
            return;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if file_type.is_dir() {
            // The vault-root papers/ tree is the paper library (its .tex is
            // outside the compilable domain), and it is by far the largest.
            if depth == 0 && name == "papers" {
                continue;
            }
            if should_ignore(name) {
                continue;
            }
            collect_tex_files(&entry.path(), depth + 1, out);
        } else if file_type.is_file() && name.to_lowercase().ends_with(".tex") {
            out.push(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write(dir: &Path, rel: &str, content: &str) -> PathBuf {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn magic_comment_resolves_relative_to_comment_file() {
        let dir = tempdir().unwrap();
        write(dir.path(), "main.tex", "\\documentclass{article} body");
        let child = write(
            dir.path(),
            "sections/ch1.tex",
            "% !TEX root = ../main.tex\nbody",
        );
        let root = resolve_root(&child, Some(dir.path()));
        assert_eq!(
            root.root_path,
            dir.path().join("main.tex").to_string_lossy()
        );
        assert!(matches!(root.source, LatexRootSource::MagicComment));
    }

    #[test]
    fn magic_comment_chain_loop_and_dangling() {
        let dir = tempdir().unwrap();
        // Chain: a → b → c (c declares nothing) resolves to c.
        write(dir.path(), "a.tex", "% !TEX root = b.tex\n");
        write(dir.path(), "b.tex", "% !TEX root = c.tex\n");
        let c = write(dir.path(), "c.tex", "\\documentclass{article}");
        let root = resolve_root(&dir.path().join("a.tex"), None);
        assert_eq!(root.root_path, c.to_string_lossy());

        // Loop: a ↔ b resolves to the file on the loop.
        write(dir.path(), "a.tex", "% !TEX root = b.tex\n");
        write(dir.path(), "b.tex", "% !TEX root = a.tex\n");
        let root = resolve_root(&dir.path().join("a.tex"), None);
        assert_eq!(root.root_path, dir.path().join("b.tex").to_string_lossy());

        // Dangling declaration abandons the strategy (b is missing) and falls
        // through: a has no indicator → fallback self.
        write(dir.path(), "a.tex", "% !TEX root = missing.tex\nbody");
        let root = resolve_root(&dir.path().join("a.tex"), None);
        assert!(matches!(root.source, LatexRootSource::FallbackSelf));
    }

    #[test]
    fn self_indicator_wins_for_standalone_doc() {
        let dir = tempdir().unwrap();
        let doc = write(dir.path(), "note.tex", "\\documentclass{article}\nbody");
        let root = resolve_root(&doc, Some(dir.path()));
        assert!(matches!(root.source, LatexRootSource::SelfIndicator));

        // A commented-out indicator does not count.
        let doc = write(dir.path(), "note2.tex", "% \\documentclass{article}\nbody");
        let root = resolve_root(&doc, Some(dir.path()));
        assert!(matches!(root.source, LatexRootSource::FallbackSelf));
    }

    #[test]
    fn scan_finds_root_via_input() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "main.tex",
            "\\documentclass{article}\n\\begin{document}\n\\input{sections/ch1}\n\\end{document}",
        );
        let child = write(dir.path(), "sections/ch1.tex", "chapter body");
        let root = resolve_root(&child, Some(dir.path()));
        assert_eq!(
            root.root_path,
            dir.path().join("main.tex").to_string_lossy()
        );
        assert!(matches!(root.source, LatexRootSource::VaultScan));
    }

    #[test]
    fn scan_follows_multilevel_chain() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "main.tex",
            "\\documentclass{book}\n\\input{parts/part1}",
        );
        write(dir.path(), "parts/part1.tex", "\\input{../sections/ch1}");
        let child = write(dir.path(), "sections/ch1.tex", "body");
        let root = resolve_root(&child, Some(dir.path()));
        assert_eq!(
            root.root_path,
            dir.path().join("main.tex").to_string_lossy()
        );
    }

    #[test]
    fn scan_resolves_include_without_extension() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "main.tex",
            "\\documentclass{article}\n\\include{sections/ch1.tex}",
        );
        let child = write(dir.path(), "sections/ch1.tex", "body");
        let root = resolve_root(&child, Some(dir.path()));
        assert_eq!(
            root.root_path,
            dir.path().join("main.tex").to_string_lossy()
        );
    }

    #[test]
    fn scan_ignores_commented_input_and_lookalike_commands() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "main.tex",
            "\\documentclass{article}\n% \\input{sections/ch1}\n\\includeonly{sections/ch1}\n\\includegraphics{x}",
        );
        let child = write(dir.path(), "sections/ch1.tex", "body");
        let root = resolve_root(&child, Some(dir.path()));
        assert!(matches!(root.source, LatexRootSource::FallbackSelf));

        // Unit level: the lookalikes produce no refs at all.
        assert!(
            parse_input_refs("\\includegraphics{x} \\includeonly{y} \\includepdf{z}").is_empty()
        );
        assert_eq!(
            parse_input_refs("\\input{a}\n\\include*{b}\n\\input [opt] {c}"),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn escaped_percent_does_not_start_comment() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "main.tex",
            "\\documentclass{article}\n100\\% \\input{ch1}",
        );
        let child = write(dir.path(), "ch1.tex", "body");
        let root = resolve_root(&child, Some(dir.path()));
        assert!(matches!(root.source, LatexRootSource::VaultScan));

        // `\\%` is a literal backslash followed by a real comment start.
        assert_eq!(truncate_at_comment("a\\\\% \\input{x}"), "a\\\\");
    }

    #[test]
    fn scan_skips_papers_and_ignored_dirs() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "papers/p1/main.tex",
            "\\documentclass{article}\n\\input{../../sections/ch1}",
        );
        write(
            dir.path(),
            "node_modules/pkg/main.tex",
            "\\documentclass{article}\n\\input{../../sections/ch1}",
        );
        let child = write(dir.path(), "sections/ch1.tex", "body");
        let root = resolve_root(&child, Some(dir.path()));
        assert!(matches!(root.source, LatexRootSource::FallbackSelf));
    }

    #[test]
    fn no_root_falls_back_to_self_and_parent_scan() {
        let dir = tempdir().unwrap();
        // No vault: the scan falls back to the file's own directory.
        write(
            dir.path(),
            "main.tex",
            "\\documentclass{article}\n\\input{ch1}",
        );
        let child = write(dir.path(), "ch1.tex", "body");
        let root = resolve_root(&child, None);
        assert!(matches!(root.source, LatexRootSource::VaultScan));

        // Truly orphaned file: fallback self.
        let orphan = write(dir.path(), "sub/orphan.tex", "body");
        let root = resolve_root(&orphan, Some(dir.path()));
        assert_eq!(root.root_path, orphan.to_string_lossy());
        assert!(matches!(root.source, LatexRootSource::FallbackSelf));
    }

    #[test]
    fn multiple_candidates_pick_shallowest_then_lexicographic() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "b.tex",
            "\\documentclass{article}\n\\input{shared}",
        );
        write(
            dir.path(),
            "a.tex",
            "\\documentclass{article}\n\\input{shared}",
        );
        write(dir.path(), "deep/d.tex", "\\input{../shared}");
        let shared = write(dir.path(), "shared.tex", "body");
        // Same BFS depth: a.tex wins lexicographically.
        let root = resolve_root(&shared, Some(dir.path()));
        assert_eq!(root.root_path, dir.path().join("a.tex").to_string_lossy());
    }

    #[test]
    fn magic_directive_parsing_shapes() {
        assert_eq!(
            parse_magic_comment("% !TEX root = main.tex\nbody"),
            Some("main.tex".to_string())
        );
        assert_eq!(
            parse_magic_comment("%! tex ROOT=../main.tex"),
            Some("../main.tex".to_string())
        );
        // Not a directive: missing value, wrong keyword, mid-line percent.
        assert_eq!(parse_magic_comment("% !TEX root ="), None);
        assert_eq!(parse_magic_comment("% !TEX task = main.tex"), None);
        assert_eq!(parse_magic_comment("code % !TEX root = main.tex"), None);
    }

    #[test]
    fn normalize_lexical_folds_dots() {
        assert_eq!(
            normalize_lexical(Path::new("/a/b/../c/./d.tex")),
            PathBuf::from("/a/c/d.tex")
        );
    }
}
