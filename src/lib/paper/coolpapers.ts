/**
 * Cool Papers (papers.cool) note fetch.
 *
 * The Host resolves the paper (Cool Papers URL / venue catalog id, then arXiv
 * id, then title), pulls its Kimi analysis and appends it to `NOTES.md`. The
 * NOTES editor toolbar triggers this and reseeds the open editor.
 */

import i18n from "@/i18n";
import { commands } from "@/lib/core/bindings";
import { errorText } from "@/lib/core/error";
import { callApiResult } from "@/lib/core/ipc";
import { notifyError, notifySuccess, notifyWarning } from "@/lib/core/notify";
import { enqueueTaskSettled, type TaskExecutorContext } from "@/lib/core/tasks";
import { arxivUrls } from "@/lib/paper/arxiv";
import { resolvePaperCatalogRel } from "@/lib/paper/library-actions";
import { notesPathForPaper } from "@/lib/paper/paths";
import type { PaperMetadata } from "@/lib/paper/types";
import { joinVaultPath, readVaultFile } from "@/lib/vault";
import { getVaultPath } from "@/lib/vault/store";
import { refreshTabNotes } from "@/lib/workspace/store";

/**
 * Append the papers.cool Kimi analysis for one paper to its NOTES.md.
 *
 * A first-time analysis is generated upstream on demand and can take up to a
 * minute, so the call runs as a JobCenter import job.
 */
export async function fetchCoolPapersNotes(meta: PaperMetadata): Promise<void> {
	const vaultPath = getVaultPath();
	if (!vaultPath) return;
	const rel = await resolvePaperCatalogRel(meta);
	if (!rel) {
		notifyError(i18n.t("app:coolPapers.resolveFailed"));
		return;
	}
	const catalogId = meta.id?.trim() || null;
	const sourceUrl = meta.source_url?.trim() || null;
	const arxivId = meta.arxiv_id ? (arxivUrls(meta.arxiv_id)?.id ?? null) : null;
	const title = meta.title?.trim() || null;
	if (!catalogId && !sourceUrl && !arxivId && !title) {
		notifyWarning(i18n.t("app:coolPapers.missingIdentifier"));
		return;
	}

	try {
		await enqueueTaskSettled({
			kind: "import",
			vaultPath,
			path: rel,
			lane: "normal",
			params: { mode: "coolNotes", catalogId, sourceUrl, arxivId, title },
		});
	} catch (e) {
		notifyError(errorText(e));
	}
}

type CoolNotesJobParams = {
	catalogId?: string | null;
	sourceUrl?: string | null;
	arxivId?: string | null;
	title?: string | null;
};

/** Executor body of a Cool Papers (`mode: "coolNotes"`) import job. */
export async function runCoolNotesImportJob(
	ctx: TaskExecutorContext,
): Promise<void> {
	const vaultPath = ctx.vaultPath;
	const rel = ctx.paperPath ?? "";
	const params = (
		ctx.params && typeof ctx.params === "object" ? ctx.params : {}
	) as CoolNotesJobParams;
	const result = await callApiResult(
		() =>
			commands.paperCoolpapersNotes({
				vaultPath,
				path: rel,
				catalogId: params.catalogId ?? null,
				sourceUrl: params.sourceUrl ?? null,
				arxivId: params.arxivId ?? null,
				title: params.title ?? null,
			}),
		{ fallback: i18n.t("app:coolPapers.fetchFailed") },
	);

	if (!result.found) {
		notifyWarning(i18n.t("app:coolPapers.notFound"));
		return;
	}
	if (!result.appended) {
		notifySuccess(i18n.t("app:coolPapers.alreadyInNotes"));
		return;
	}

	const paperDir = joinVaultPath(vaultPath, rel);
	try {
		const content = await readVaultFile(notesPathForPaper(paperDir));
		refreshTabNotes(paperDir, content);
	} catch {
		// Reseeding the open editor is best-effort; the file is already written.
	}
	notifySuccess(i18n.t("app:coolPapers.appended"));
}
