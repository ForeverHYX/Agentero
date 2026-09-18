//! Shared name/path helpers for the wiki domain.
//!
//! `doctor`, `rename`, `resolve`, and `index` previously kept private copies of
//! these helpers; the single implementations here keep link rewriting, fuzzy
//! matching, and search naming from drifting apart.

use crate::features::wiki::models::InternalLinkSyntax;
use std::path::{Component, Path};

const MARKDOWN_EXTENSIONS: [&str; 3] = [".markdown", ".mdx", ".md"];

/// Collapse whitespace runs and lowercase for fuzzy key matching.
pub(crate) fn normalize_key(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// File stem of a vault-relative path, falling back to the full input.
pub(crate) fn stem_of(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .to_string()
}

/// Strip a markdown extension (`.markdown` / `.mdx` / `.md`) case-sensitively.
///
/// Targets come from the document index, where extensions are stored with
/// their real on-disk casing. For display names over arbitrary casing use
/// [`without_markdown_extension`].
pub(crate) fn strip_markdown_extension(path: &str) -> &str {
    MARKDOWN_EXTENSIONS
        .iter()
        .find_map(|extension| path.strip_suffix(extension))
        .unwrap_or(path)
}

/// Strip a markdown extension case-insensitively (`.MD`, `.Markdown`, ...).
///
/// Search candidates surface user-created file names verbatim, so upper-case
/// extensions must still be stripped here.
pub(crate) fn without_markdown_extension(path: &str) -> String {
    let lower = path.to_ascii_lowercase();
    for extension in MARKDOWN_EXTENSIONS {
        if lower.ends_with(extension) {
            return path[..path.len() - extension.len()].to_string();
        }
    }
    path.to_string()
}

fn component_name(component: Component<'_>) -> Option<&str> {
    match component {
        Component::Normal(value) => value.to_str(),
        _ => None,
    }
}

/// Shortest `../`-style Markdown link from `source` to vault-relative `target`.
fn markdown_relative_target(source: &str, target: &str) -> String {
    let source_parent = Path::new(source).parent().unwrap_or_else(|| Path::new(""));
    let source_parts = source_parent
        .components()
        .filter_map(component_name)
        .collect::<Vec<_>>();
    let target_parts = Path::new(target)
        .components()
        .filter_map(component_name)
        .collect::<Vec<_>>();
    let common = source_parts
        .iter()
        .zip(&target_parts)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts = Vec::new();
    parts.extend(std::iter::repeat_n("..", source_parts.len() - common));
    parts.extend(target_parts[common..].iter().copied());
    if parts.is_empty() {
        Path::new(target)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(target)
            .to_string()
    } else {
        parts.join("/")
    }
}

/// Replacement text for a link target whose document moved to `final_target`.
///
/// Wikilinks keep the vault path without extension; Markdown links become
/// source-relative.
pub(crate) fn replacement_target(
    syntax: &InternalLinkSyntax,
    target_raw: &str,
    source: &str,
    final_target: &str,
) -> String {
    if target_raw.is_empty() && source == final_target {
        return String::new();
    }
    match syntax {
        InternalLinkSyntax::Wikilink => strip_markdown_extension(final_target).to_string(),
        InternalLinkSyntax::Markdown => markdown_relative_target(source, final_target),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_target_keeps_shared_prefix_and_backtracks_disjoint_dirs() {
        assert_eq!(
            markdown_relative_target("notes/Source.md", "notes/Target.md"),
            "Target.md"
        );
        assert_eq!(
            markdown_relative_target("notes/sub/Source.md", "notes/Target.md"),
            "../Target.md"
        );
        assert_eq!(
            markdown_relative_target("notes/a/Source.md", "archive/b/Target.md"),
            "../../archive/b/Target.md"
        );
        assert_eq!(
            markdown_relative_target("Source.md", "notes/Target.md"),
            "notes/Target.md"
        );
        assert_eq!(
            markdown_relative_target("notes/Source.md", "Target.md"),
            "../Target.md"
        );
    }

    #[test]
    fn extension_stripping_differs_only_in_extension_casing() {
        assert_eq!(strip_markdown_extension("notes/a.md"), "notes/a");
        assert_eq!(strip_markdown_extension("notes/a.markdown"), "notes/a");
        assert_eq!(strip_markdown_extension("notes/a.mdx"), "notes/a");
        assert_eq!(strip_markdown_extension("notes/a.txt"), "notes/a.txt");
        assert_eq!(strip_markdown_extension("notes/a.MD"), "notes/a.MD");
        assert_eq!(without_markdown_extension("notes/a.md"), "notes/a");
        assert_eq!(without_markdown_extension("notes/a.MD"), "notes/a");
        assert_eq!(without_markdown_extension("notes/a.Markdown"), "notes/a");
        assert_eq!(without_markdown_extension("notes/a.txt"), "notes/a.txt");
    }

    #[test]
    fn normalize_key_and_stem_of_match_resolver_expectations() {
        assert_eq!(normalize_key("  Short   NAME "), "short name");
        assert_eq!(stem_of("notes/a.b.md"), "a.b");
        assert_eq!(stem_of("a.md"), "a");
        assert_eq!(stem_of("weird"), "weird");
    }

    #[test]
    fn wikilink_replacement_strips_extension_and_empty_target_self_link_stays_empty() {
        assert_eq!(
            replacement_target(
                &InternalLinkSyntax::Wikilink,
                "notes/Target",
                "notes/Source.md",
                "notes/Target.md"
            ),
            "notes/Target"
        );
        assert_eq!(
            replacement_target(
                &InternalLinkSyntax::Markdown,
                "",
                "notes/Source.md",
                "notes/Source.md"
            ),
            ""
        );
    }
}
