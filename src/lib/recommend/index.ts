/**
 * arXiv daily recommendation — Host IPC.
 *
 * Ranks today's arXiv papers against the Vault library (embedding similarity
 * weighted toward recently added papers). The Host reuses its stored same-day
 * run unless `force` is set, so calling this on vault open is cheap.
 */

import {
	commands,
	type ProbeEmbeddingResult,
	type RecommendItem,
	type RecommendResult,
} from "@/lib/core/bindings";
import { errorText } from "@/lib/core/error";
import { callApi } from "@/lib/core/ipc";

/** Read models come straight from the generated wire contract. */
export type { ProbeEmbeddingResult, RecommendItem, RecommendResult };

/** Host marker for "no embedding endpoint configured" (Settings → Agent). */
export const ERR_NO_EMBEDDING = "recommend.no_embedding";
/** Host marker for "library has no abstracts to compare against". */
export const ERR_EMPTY_CORPUS = "recommend.empty_corpus";
/** Host marker for "the arXiv feeds returned nothing usable". */
export const ERR_NO_CANDIDATES = "recommend.no_candidates";
/** Host marker for "the embedding endpoint did not respond to the probe". */
export const ERR_PROBE_FAILED = "recommend.probe_failed";

export function isNoEmbeddingError(error: unknown): boolean {
	return errorMessage(error) === ERR_NO_EMBEDDING;
}

export function isEmptyCorpusError(error: unknown): boolean {
	return errorMessage(error) === ERR_EMPTY_CORPUS;
}

export function isNoCandidatesError(error: unknown): boolean {
	return errorMessage(error) === ERR_NO_CANDIDATES;
}

export function isProbeFailedError(error: unknown): boolean {
	return errorMessage(error) === ERR_PROBE_FAILED;
}

/**
 * Liveness check for the configured embedding endpoint.
 *
 * Pass any subset of `baseUrl` / `apiKey` / `model` to override the stored
 * values (the Agent settings pane tests its draft before committing). The
 * Host treats an empty string or the `*` mask as "use the stored value".
 */
export async function probeEmbedding(opts?: {
	baseUrl?: string;
	apiKey?: string;
	model?: string;
}): Promise<ProbeEmbeddingResult> {
	return callApi(
		() =>
			commands.probeEmbedding({
				baseUrl: opts?.baseUrl ?? null,
				apiKey: opts?.apiKey ?? null,
				model: opts?.model ?? null,
			}),
		{ fallback: "recommend.probe_failed" },
	);
}

function errorMessage(error: unknown): string {
	return errorText(error).trim();
}

export async function recommendArxiv(opts: {
	vaultPath: string;
	categories?: string[];
	topN?: number;
	force?: boolean;
}): Promise<RecommendResult> {
	return callApi(
		() =>
			commands.recommendArxiv({
				vaultPath: opts.vaultPath,
				categories: opts.categories ?? null,
				topN: opts.topN ?? null,
				force: opts.force ?? false,
			}),
		{ fallback: "recommend.failed" },
	);
}

/** Stored run, or null when this vault has never computed one. */
export async function recommendArxivLast(
	vaultPath: string,
): Promise<RecommendResult | null> {
	const result = await callApi(
		() => commands.recommendArxivLast({ vaultPath }),
		{ fallback: "recommend.failed" },
	);
	return result ?? null;
}
