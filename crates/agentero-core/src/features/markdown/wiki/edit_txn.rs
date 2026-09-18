//! Shared edit-transaction primitives for the wiki rename domains.
//!
//! Both the file-move transaction ([`super::rename`]) and the heading rename
//! transaction ([`super::heading_rename`]) plan byte-range edits against
//! snapshot-hashed sources and commit them as atomic writes with rollback.
//! This module owns that shared skeleton — edit records, in-place editing,
//! preflight validation, source verification, and the write/restore loop.
//! The domain modules keep only their planning logic and rollback scope.

use crate::features::wiki::models::{WikiRenameErrorCode, WikiRenameRollback};
use crate::features::wiki::rename::WikiRenameError;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use uuid::Uuid;

/// One byte-range replacement checked against the bytes it expects to overlap.
#[derive(Debug, Clone)]
pub(crate) struct PlannedEdit {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) expected: String,
    pub(crate) replacement: String,
}

/// One source file participating in a transaction: the content snapshot the
/// plan was built from plus the edits to re-apply atomically at commit time.
#[derive(Debug, Clone)]
pub(crate) struct PlannedSource {
    /// Path the preflight verification re-reads (the pre-move location for
    /// external-repair plans; identical to `final_path` otherwise).
    pub(crate) current_path: String,
    /// Path the rewritten content is committed to.
    pub(crate) final_path: String,
    pub(crate) original_content: String,
    pub(crate) original_hash: String,
    pub(crate) edits: Vec<PlannedEdit>,
}

impl PlannedSource {
    /// A source rewritten in place, without any filesystem move.
    pub(crate) fn in_place(
        path: String,
        original_content: String,
        edits: Vec<PlannedEdit>,
    ) -> Self {
        let original_hash = content_hash(&original_content);
        Self {
            current_path: path.clone(),
            final_path: path,
            original_content,
            original_hash,
            edits,
        }
    }
}

/// Apply `edits` (already sorted descending by start) to `content`.
pub(crate) fn apply_edits(content: &str, edits: &[PlannedEdit]) -> String {
    let mut rewritten = content.to_string();
    for edit in edits {
        rewritten.replace_range(edit.start..edit.end, &edit.replacement);
    }
    rewritten
}

/// Sort edits into apply order and reject ranges that overlap, exceed the
/// source bounds, or no longer match their expected bytes. `label` names the
/// calling domain in the error message ("planned" / "heading").
pub(crate) fn validate_edits(
    path: &str,
    content: &str,
    edits: &mut [PlannedEdit],
    label: &str,
) -> Result<(), WikiRenameError> {
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.start));
    if edits.iter().any(|edit| {
        edit.start > edit.end
            || edit.end > content.len()
            || !content.is_char_boundary(edit.start)
            || !content.is_char_boundary(edit.end)
            || content.get(edit.start..edit.end) != Some(edit.expected.as_str())
    }) || edits.windows(2).any(|pair| pair[0].start < pair[1].end)
    {
        return Err(WikiRenameError::new(
            WikiRenameErrorCode::OverlappingEdits,
            format!("{label} edits overlap or exceed source bounds in {path}"),
        ));
    }
    Ok(())
}

/// Re-read every planned source and confirm it still hashes to the snapshot
/// taken at plan time.
pub(crate) fn verify_sources_unchanged(
    vault_root: &Path,
    sources: &[PlannedSource],
    label: &str,
) -> Result<(), WikiRenameError> {
    for source in sources {
        let current =
            fs::read_to_string(vault_root.join(&source.current_path)).map_err(|error| {
                WikiRenameError::new(
                    WikiRenameErrorCode::SourceChanged,
                    format!(
                        "could not re-read {label} source {}: {error}",
                        source.current_path
                    ),
                )
            })?;
        if content_hash(&current) != source.original_hash {
            return Err(WikiRenameError::new(
                WikiRenameErrorCode::SourceChanged,
                format!("{label} source changed: {}", source.current_path),
            ));
        }
    }
    Ok(())
}

/// A write-loop failure after the domain rollback already ran.
pub(crate) struct PlannedWriteFailure {
    pub(crate) rollback: WikiRenameRollback,
    /// Index into `sources` of the source that failed (or was simulated to).
    pub(crate) source_index: usize,
    /// Underlying atomic-write error; `None` for simulated failures.
    pub(crate) error: Option<String>,
}

/// Write every source's rewritten content atomically, in order. On the first
/// failure the caller-supplied `rollback` runs with the so-far-written
/// sources before the failure is returned; on success the written sources
/// are returned for any later commit-stage rollback.
pub(crate) fn write_all_sources<'a, F>(
    vault_root: &Path,
    sources: &'a [PlannedSource],
    fail_write_at: Option<usize>,
    mut rollback: F,
) -> Result<Vec<&'a PlannedSource>, PlannedWriteFailure>
where
    F: FnMut(&[&'a PlannedSource]) -> WikiRenameRollback,
{
    let mut written: Vec<&PlannedSource> = Vec::new();
    for (write_index, source) in sources.iter().enumerate() {
        if fail_write_at == Some(write_index) {
            return Err(PlannedWriteFailure {
                rollback: rollback(&written),
                source_index: write_index,
                error: None,
            });
        }
        let rewritten = apply_edits(&source.original_content, &source.edits);
        if let Err(error) = atomic_write(&vault_root.join(&source.final_path), rewritten.as_bytes())
        {
            return Err(PlannedWriteFailure {
                rollback: rollback(&written),
                source_index: write_index,
                error: Some(error),
            });
        }
        written.push(source);
    }
    Ok(written)
}

/// Restore the original content of `written` sources in reverse write order.
/// Returns whether every restore write succeeded.
pub(crate) fn restore_written_sources(vault_root: &Path, written: &[&PlannedSource]) -> bool {
    let mut complete = true;
    for source in written.iter().rev() {
        if atomic_write(
            &vault_root.join(&source.final_path),
            source.original_content.as_bytes(),
        )
        .is_err()
        {
            complete = false;
        }
    }
    complete
}

pub fn content_hash(content: &str) -> String {
    hex::encode(Sha256::digest(content.as_bytes()))
}

pub fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), String> {
    path.parent()
        .ok_or_else(|| format!("{} has no parent", path.display()))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("markdown");
    // `.agentero-rename-` marks the temp for the vault watcher (content
    // modify, not a user rename); the `.tmp` suffix keeps sync scans away.
    let opts = crate::fs::AtomicOpts {
        temp_name: Some(format!(".{name}.agentero-rename-{}.tmp", Uuid::new_v4())),
        ..Default::default()
    };
    crate::fs::atomic_write_with(path, contents, &opts).map_err(|error| error.to_string())
}
