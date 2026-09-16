//! NOTES.md alias maintenance: title renames keep old `[[...]]` links
//! resolving by appending the new title to frontmatter aliases and updating
//! placeholder H1 titles.

use std::fs;
use std::path::Path;

/// Append the new title to NOTES.md frontmatter aliases (best-effort; keeps
/// old aliases so existing `[[...]]` links keep resolving).
pub fn append_title_alias_best_effort(vault_root: &Path, rel_path: &str, title: &str) {
    sync_notes_title_and_alias(vault_root, rel_path, "", title);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NotesPatchReport {
    pub alias_added: Option<String>,
    pub h1_action: H1Action,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum H1Action {
    Replaced {
        old_h1: String,
        new_h1: String,
        reason: &'static str,
    },
    Inserted {
        new_h1: String,
    },
    Preserved {
        h1: String,
        reason: &'static str,
    },
    Unchanged,
}

/// Synchronize NOTES.md when a paper's title changes:
/// 1. Appends `new_title` to YAML frontmatter aliases (keeps old aliases for wiki links).
/// 2. If the note heading matches `old_title` or is a placeholder, replaces it with `# {new_title}`.
pub fn sync_notes_title_and_alias(
    vault_root: &Path,
    rel_path: &str,
    old_title: &str,
    new_title: &str,
) {
    let notes_path = vault_root.join(rel_path).join("NOTES.md");
    let body = match fs::read_to_string(&notes_path) {
        Ok(b) => b,
        Err(err) => {
            log::debug!(
                target: "agentero::notes",
                "sync_notes_title_and_alias: NOTES.md not found or unreadable at {:?}: {err}",
                notes_path
            );
            return;
        }
    };
    let (updated, report) = patch_notes_title_and_alias_with_report(&body, old_title, new_title);
    if updated != body {
        match fs::write(&notes_path, &updated) {
            Ok(_) => {
                log::info!(
                    target: "agentero::notes",
                    "NOTES.md synced for {:?}: old_title={:?}, new_title={:?}, h1={:?}, alias={:?}",
                    rel_path,
                    old_title,
                    new_title,
                    report.h1_action,
                    report.alias_added
                );
            }
            Err(err) => {
                log::error!(
                    target: "agentero::notes",
                    "failed to write synced NOTES.md for {:?}: {err}",
                    rel_path
                );
            }
        }
    } else {
        log::debug!(
            target: "agentero::notes",
            "NOTES.md unchanged for {:?}: h1={:?}",
            rel_path,
            report.h1_action
        );
    }
}

#[cfg(test)]
pub(crate) fn patch_notes_title_and_alias(body: &str, old_title: &str, new_title: &str) -> String {
    let (updated, _) = patch_notes_title_and_alias_with_report(body, old_title, new_title);
    updated
}

pub(crate) fn patch_notes_title_and_alias_with_report(
    body: &str,
    old_title: &str,
    new_title: &str,
) -> (String, NotesPatchReport) {
    use crate::features::wiki::frontmatter as fm;
    let new_title = new_title.trim();
    if new_title.is_empty() {
        return (
            body.to_string(),
            NotesPatchReport {
                alias_added: None,
                h1_action: H1Action::Unchanged,
            },
        );
    }

    let (frontmatter_end, existing) = fm::parse_frontmatter_aliases(body);
    let mut updated = body.to_string();
    let mut alias_added = None;

    let trimmed_old = old_title.trim();
    let mut merged = existing.clone();
    if !trimmed_old.is_empty()
        && !merged.iter().any(|a| a == trimmed_old)
        && trimmed_old != new_title
    {
        merged.push(trimmed_old.to_string());
    }
    if !merged.iter().any(|a| a == new_title) {
        merged.push(new_title.to_string());
        alias_added = Some(new_title.to_string());
    }

    if merged != existing {
        let next = if frontmatter_end == 0 {
            fm::prepend_new_aliases(&updated, &merged)
        } else if merged.len() >= 2 {
            fm::patch_aliases(&updated, &merged)
        } else {
            Err("cannot patch aliases".into())
        };
        if let Ok(next) = next {
            updated = next;
        }
    }

    let (fm_end, _) = fm::parse_frontmatter_aliases(&updated);
    let (frontmatter_part, content_part) = updated.split_at(fm_end);

    let has_user_content = has_user_notes(content_part, trimmed_old);
    let mut h1_action = H1Action::Unchanged;

    let lines: Vec<&str> = content_part.lines().collect();
    let mut h1_line_idx = None;
    for (idx, line) in lines.iter().enumerate() {
        if line.starts_with("# ") {
            h1_line_idx = Some(idx);
            break;
        }
    }

    let mut new_lines = Vec::with_capacity(lines.len() + 1);

    if let Some(idx) = h1_line_idx {
        let existing_h1 = lines[idx][2..].trim();
        let (should_replace, reason) = if !trimmed_old.is_empty() && existing_h1 == trimmed_old {
            (true, "exact match with old title")
        } else if !trimmed_old.is_empty() && matches_old_title(existing_h1, trimmed_old) {
            (true, "stem / filename / URL match with old title")
        } else if is_generic_placeholder(existing_h1) {
            (true, "generic placeholder heading")
        } else if !has_user_content {
            (true, "untouched placeholder note")
        } else {
            (false, "user custom heading preserved")
        };

        if should_replace {
            if existing_h1 != new_title {
                h1_action = H1Action::Replaced {
                    old_h1: existing_h1.to_string(),
                    new_h1: new_title.to_string(),
                    reason,
                };
                for (i, line) in lines.iter().enumerate() {
                    if i == idx {
                        new_lines.push(format!("# {new_title}"));
                    } else {
                        new_lines.push(line.to_string());
                    }
                }
            } else {
                h1_action = H1Action::Unchanged;
                for line in &lines {
                    new_lines.push(line.to_string());
                }
            }
        } else {
            h1_action = H1Action::Preserved {
                h1: existing_h1.to_string(),
                reason,
            };
            for line in &lines {
                new_lines.push(line.to_string());
            }
        }
    } else if !has_user_content {
        let mut replaced_raw = false;
        for line in &lines {
            let trimmed = line.trim();
            if !replaced_raw
                && !trimmed.is_empty()
                && (matches_old_title(trimmed, trimmed_old)
                    || is_generic_placeholder(trimmed)
                    || strip_pdf_ext(trimmed) == strip_pdf_ext(trimmed_old))
            {
                new_lines.push(format!("# {new_title}"));
                replaced_raw = true;
                h1_action = H1Action::Replaced {
                    old_h1: trimmed.to_string(),
                    new_h1: new_title.to_string(),
                    reason: "raw placeholder line replaced",
                };
            } else {
                new_lines.push(line.to_string());
            }
        }
        if !replaced_raw {
            new_lines.insert(0, format!("# {new_title}"));
            h1_action = H1Action::Inserted {
                new_h1: new_title.to_string(),
            };
        }
    } else {
        for line in &lines {
            new_lines.push(line.to_string());
        }
    }

    let mut new_content = new_lines.join("\n");
    if content_part.ends_with('\n') || (content_part.is_empty() && !new_content.is_empty()) {
        new_content.push('\n');
    }

    (
        format!("{frontmatter_part}{new_content}"),
        NotesPatchReport {
            alias_added,
            h1_action,
        },
    )
}

fn strip_pdf_ext(s: &str) -> &str {
    let trimmed = s.trim();
    if let Some(stripped) = trimmed.strip_suffix(".pdf") {
        stripped.trim()
    } else if let Some(stripped) = trimmed.strip_suffix(".PDF") {
        stripped.trim()
    } else {
        trimmed
    }
}

fn file_stem_from_url_or_path(s: &str) -> &str {
    let trimmed = s.trim();
    let segment = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    strip_pdf_ext(segment)
}

fn is_generic_placeholder(s: &str) -> bool {
    let lower = s.trim().to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "" | "pdf"
            | "untitled"
            | "untitled paper"
            | "placeholder"
            | "placeholder title"
            | "notes"
            | "paper"
            | "未命名"
            | "未命名论文"
            | "无标题"
    )
}

fn matches_old_title(h1_text: &str, old_title: &str) -> bool {
    let h1_trim = h1_text.trim();
    let old_trim = old_title.trim();
    if h1_trim.is_empty() || old_trim.is_empty() {
        return false;
    }
    if h1_trim.eq_ignore_ascii_case(old_trim) {
        return true;
    }
    let h1_stem = strip_pdf_ext(h1_trim);
    let old_stem = strip_pdf_ext(old_trim);
    if !h1_stem.is_empty() && h1_stem.eq_ignore_ascii_case(old_stem) {
        return true;
    }
    let h1_url_stem = file_stem_from_url_or_path(h1_trim);
    let old_url_stem = file_stem_from_url_or_path(old_trim);
    if !h1_url_stem.is_empty() && h1_url_stem.eq_ignore_ascii_case(old_url_stem) {
        return true;
    }
    false
}

fn has_user_notes(content_part: &str, old_title: &str) -> bool {
    for line in content_part.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("# ") {
            continue;
        }
        if trimmed.starts_with('>') {
            continue;
        }
        if trimmed == "---" || trimmed == "***" {
            continue;
        }
        if matches_old_title(trimmed, old_title) || is_generic_placeholder(trimmed) {
            continue;
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_patch_notes_title_and_alias_updates_h1_and_aliases() {
        let original = r#"---
aliases:
  - "2024-134-paper.pdf"
  - "21PP"
---
# 2024-134-paper.pdf

User notes here.
"#;
        let result = patch_notes_title_and_alias(
            original,
            "2024-134-paper.pdf",
            "ShapFuzz: Efficient Fuzzing via Shapley-Guided Byte Selection",
        );
        assert!(result.contains("ShapFuzz: Efficient Fuzzing via Shapley-Guided Byte Selection"));
        assert!(result.contains("# ShapFuzz: Efficient Fuzzing via Shapley-Guided Byte Selection"));
        assert!(!result.contains("# 2024-134-paper.pdf"));
        assert!(result.contains("- \"2024-134-paper.pdf\""));
        assert!(result.contains("User notes here."));
    }

    #[test]
    fn test_patch_notes_raw_filename_without_h1_updates_to_h1() {
        let original = "2024-134-paper.pdf\n";
        let result = patch_notes_title_and_alias(
            original,
            "2024-134-paper.pdf",
            "ShapFuzz: Efficient Fuzzing via Shapley-Guided Byte Selection",
        );
        assert!(result.contains("# ShapFuzz: Efficient Fuzzing via Shapley-Guided Byte Selection"));
        assert!(!result.contains("\n2024-134-paper.pdf\n"));
        assert!(result.contains("- \"2024-134-paper.pdf\""));
    }

    #[test]
    fn test_patch_notes_url_old_title_matches_stem() {
        let original = r#"---
aliases:
  - "2024-134-paper"
---
# 2024-134-paper.pdf
"#;
        let result = patch_notes_title_and_alias(
            original,
            "https://www.ndss-symposium.org/wp-content/uploads/2024-134-paper.pdf",
            "ShapFuzz: Efficient Fuzzing via Shapley-Guided Byte Selection",
        );
        assert!(result.contains("# ShapFuzz: Efficient Fuzzing via Shapley-Guided Byte Selection"));
        assert!(!result.contains("# 2024-134-paper.pdf"));
    }

    #[test]
    fn test_patch_notes_preserves_custom_user_h1() {
        let original = r#"---
aliases:
  - "old title"
---
# My Custom Analysis Header

Notes.
"#;
        let result = patch_notes_title_and_alias(original, "old title", "New Title");
        assert!(result.contains("# My Custom Analysis Header"));
        assert!(result.contains("New Title"));
    }

    #[test]
    fn test_patch_notes_untouched_note_replaces_any_h1() {
        let original = r#"---
aliases:
  - "placeholder"
---
# Untitled
"#;
        let result = patch_notes_title_and_alias(original, "", "Recognized Title");
        assert!(result.contains("# Recognized Title"));
        assert!(!result.contains("# Untitled"));
    }

    #[test]
    fn test_patch_notes_with_abstract_blockquote_preserves_blockquote() {
        let original = r#"---
aliases:
  - "2024-134-paper.pdf"
---
# 2024-134-paper.pdf

> This paper introduces ShapFuzz, an efficient greybox fuzzer.
"#;
        let result = patch_notes_title_and_alias(
            original,
            "2024-134-paper.pdf",
            "ShapFuzz: Efficient Fuzzing via Shapley-Guided Byte Selection",
        );
        assert!(result.contains("# ShapFuzz: Efficient Fuzzing via Shapley-Guided Byte Selection"));
        assert!(result.contains("> This paper introduces ShapFuzz, an efficient greybox fuzzer."));
    }
}
