/**
 * Ephemeral selection-ask prompt for Plaza surfaces (RSS feed detail, etc.).
 * Parallel to PDF ask (`buildPdfAskPrompt`) but without page geometry.
 */

import type { PdfAskThread } from "@/lib/pdf/ask/types";

/**
 * Build a single-turn prompt for a text-surface selection ask (plaza feed
 * detail, proxied web papers, …).
 */
export function buildPlazaAskPrompt(
	thread: PdfAskThread,
	latestUserQuestion: string,
	opts?: { title?: string; url?: string | null; surface?: "feed" | "web" },
): string {
	const quote = thread.anchor.quote?.trim();
	const history = thread.messages
		.filter((m) => m.role === "user" || m.role === "assistant")
		.slice(0, -1)
		.map((m) => `${m.role === "user" ? "User" : "Assistant"}: ${m.content}`)
		.join("\n\n");

	const parts = [
		opts?.surface === "web"
			? "You are helping the user read a web page in Agentero."
			: "You are helping the user read a research feed item in Agentero.",
	];
	const title = opts?.title?.trim();
	if (title) parts.push(`Item title: ${title}`);
	const url = opts?.url?.trim();
	if (url) parts.push(`Item URL: ${url}`);
	if (quote) {
		parts.push("Quoted text from the item:", `> ${quote}`);
	}
	if (history) {
		parts.push("Earlier turns in this selection thread:", history);
	}
	const q = latestUserQuestion.trim();
	parts.push(
		"User question:",
		q || "(no text)",
		"Answer based on the quote and prior turns when possible. Be concise. If uncertain, say so.",
	);
	return parts.join("\n\n");
}
