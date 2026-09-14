/**
 * Shared lifecycle of an ephemeral selection Ask (quick chat) thread.
 *
 * Lifted from the Plaza feed selection hook so any text surface (plaza detail,
 * proxied web papers, …) gets the same run machinery: in-memory `PdfAskThread`
 * (nothing writes marks/), `runOnce({workflow: "free"})` + `attachAgentRun`
 * streaming, resend-from-turn, stop / hide / delete, and IPC teardown.
 * Surfaces keep their own selection capture and pass a `buildPrompt` that
 * stamps their context (title / URL / surface wording).
 */

import type { UnlistenFn } from "@tauri-apps/api/event";
import {
	type RefObject,
	useCallback,
	useEffect,
	useRef,
	useState,
} from "react";
import { useTranslation } from "react-i18next";
import {
	attachAgentRun,
	cancelAgentRun,
	disposeAgentRun,
	listAgents,
	runOnce,
} from "@/lib/agent";
import { errorText } from "@/lib/core/error";
import { notifyError } from "@/lib/core/notify";
import { createEmptyThread, newMessageId } from "@/lib/pdf/ask";
import { threadHasUserQuestion } from "@/lib/pdf/ask/schema";
import type { PdfAskThread } from "@/lib/pdf/ask/types";
import { loadSettings } from "@/lib/settings";
import { resolveTranslateAgent } from "@/lib/translate";
import { getVaultPath } from "@/lib/vault/store";

export type SelectionAskState<S> = {
	thread: PdfAskThread;
	/** Surface-specific placement info for the ask card. */
	screen: S;
};

/** Open an ephemeral ask thread anchored at a selection quote. */
export function createSelectionAskThread(
	sourcePath: string,
	quote: string,
): PdfAskThread {
	return createEmptyThread({
		paperPath: sourcePath,
		anchor: { page: 1, rects: [], quote, trigger: "selection" },
	});
}

export function useSelectionAsk<S>({
	buildPrompt,
	activeSessionRef,
}: {
	/** Surface context (title / URL / wording) stamped around the thread. */
	buildPrompt: (thread: PdfAskThread, latestUserQuestion: string) => string;
	/**
	 * Single in-flight agent run per surface, shared with the translate
	 * cluster; parent-owned so either can cancel the other's session token.
	 */
	activeSessionRef: RefObject<string | null>;
}) {
	const { t } = useTranslation("viewer");
	const [ask, setAsk] = useState<SelectionAskState<S> | null>(null);
	const [streaming, setStreaming] = useState(false);
	const [askError, setAskError] = useState<string | null>(null);

	const askRef = useRef<SelectionAskState<S> | null>(null);
	askRef.current = ask;

	const runDisposedRef = useRef(false);
	const runUnsubsRef = useRef<UnlistenFn[] | null>(null);
	const askSessionRef = useRef<string | null>(null);

	useEffect(() => {
		runDisposedRef.current = false;
		return () => {
			disposeAgentRun({
				disposedRef: runDisposedRef,
				unsubsRef: runUnsubsRef,
				sessionRef: askSessionRef,
				activeSessionRef,
			});
		};
	}, [activeSessionRef]);

	/** Drop the ask chrome without cancelling (surface navigation reset). */
	const resetAsk = useCallback(() => {
		setAsk(null);
		setAskError(null);
		setStreaming(false);
	}, []);

	const resolveAskAgent = useCallback(async () => {
		const registry = await listAgents().catch(() => null);
		const resolved = resolveTranslateAgent(loadSettings().pdfAsk, registry);
		if (!resolved.agentId) {
			const msg = t("pdfAsk.noAgent");
			notifyError(msg);
			setAskError(msg);
			return null;
		}
		return resolved;
	}, [t]);

	const upsertAskThread = useCallback((thread: PdfAskThread) => {
		setAsk((prev) => (prev ? { ...prev, thread } : prev));
	}, []);

	const sendToThread = useCallback(
		async (
			thread: PdfAskThread,
			question: string,
			agentOpts?: { agentId?: string; modelId?: string },
			baseMessages?: PdfAskThread["messages"],
		) => {
			const threadId = thread.id;
			if (!question.trim()) return;
			const userMsg = {
				id: newMessageId(),
				role: "user" as const,
				content: question,
				createdAt: new Date().toISOString(),
			};
			const prior = baseMessages ?? thread.messages;
			const withUser: PdfAskThread = {
				...thread,
				status: "open",
				messages: [...prior, userMsg],
				updatedAt: new Date().toISOString(),
			};
			upsertAskThread(withUser);
			setAskError(null);
			setStreaming(true);

			const assistantId = newMessageId();
			const prompt = buildPrompt(withUser, question);
			try {
				const accepted = await runOnce({
					prompt,
					agentId: agentOpts?.agentId,
					modelId: agentOpts?.modelId,
					vaultPath: getVaultPath() ?? undefined,
					workflow: "free",
					permissionMode: "auto",
					hideFromChatHistory: true,
				});
				const withAssistant: PdfAskThread = {
					...withUser,
					messages: [
						...withUser.messages,
						{
							id: assistantId,
							role: "assistant",
							content: "",
							createdAt: new Date().toISOString(),
							agentSessionId: accepted.sessionId,
						},
					],
				};
				await attachAgentRun({
					accepted,
					disposedRef: runDisposedRef,
					unsubsRef: runUnsubsRef,
					sessionRef: askSessionRef,
					activeSessionRef,
					onArmed: () => upsertAskThread(withAssistant),
					onStream: (ev) => {
						setAsk((prev) => {
							if (!prev || prev.thread.id !== threadId) return prev;
							const msgs = [...prev.thread.messages];
							const last = msgs[msgs.length - 1];
							if (last?.id !== assistantId) return prev;
							msgs[msgs.length - 1] = {
								...last,
								content: last.content + ev.chunk,
							};
							return { ...prev, thread: { ...prev.thread, messages: msgs } };
						});
					},
					onCompleted: (ev) => {
						setAsk((prev) => {
							if (!prev || prev.thread.id !== threadId) return prev;
							const msgs = [...prev.thread.messages];
							const last = msgs[msgs.length - 1];
							if (last?.id === assistantId) {
								msgs[msgs.length - 1] = {
									...last,
									content: ev.content || last.content,
									sources: (ev.sources ?? []).map((uri) => ({ uri })),
								};
							}
							return {
								...prev,
								thread: {
									...prev.thread,
									messages: msgs,
									updatedAt: new Date().toISOString(),
								},
							};
						});
					},
					onFailed: (ev) => {
						setAskError(ev.error || t("pdfAsk.agentFailed"));
						setAsk((prev) => {
							if (!prev || prev.thread.id !== threadId) return prev;
							return {
								...prev,
								thread: {
									...prev.thread,
									messages: prev.thread.messages.filter(
										(m) => m.id !== assistantId,
									),
								},
							};
						});
					},
					onSettled: () => setStreaming(false),
				});
			} catch (e) {
				setStreaming(false);
				setAskError(e instanceof Error ? e.message : t("pdfAsk.agentFailed"));
			}
		},
		[buildPrompt, upsertAskThread, activeSessionRef, t],
	);

	const sendAskQuestion = useCallback(
		(question: string) => {
			const current = askRef.current;
			if (!current) return;
			void (async () => {
				try {
					const resolved = await resolveAskAgent();
					if (!resolved) return;
					void sendToThread(current.thread, question, {
						agentId: resolved.agentId,
						modelId: resolved.modelId,
					});
				} catch (e) {
					const message = errorText(e);
					notifyError(message);
					setAskError(message);
				}
			})();
		},
		[resolveAskAgent, sendToThread],
	);

	const resendAskQuestion = useCallback(
		(messageId: string, question: string) => {
			const current = askRef.current;
			if (!current) return;
			const index = current.thread.messages.findIndex(
				(m) => m.id === messageId && m.role === "user",
			);
			if (index < 0) return;
			const baseMessages = current.thread.messages.slice(0, index);
			void (async () => {
				try {
					const resolved = await resolveAskAgent();
					if (!resolved) return;
					void sendToThread(
						current.thread,
						question,
						{
							agentId: resolved.agentId,
							modelId: resolved.modelId,
						},
						baseMessages,
					);
				} catch (e) {
					const message = errorText(e);
					notifyError(message);
					setAskError(message);
				}
			})();
		},
		[resolveAskAgent, sendToThread],
	);

	const stopAskStreaming = useCallback(() => {
		const sid = askSessionRef.current;
		if (!sid) return;
		askSessionRef.current = null;
		if (activeSessionRef.current === sid) activeSessionRef.current = null;
		void cancelAgentRun(sid).catch(() => undefined);
		setStreaming(false);
	}, [activeSessionRef]);

	const hideAsk = useCallback(() => {
		stopAskStreaming();
		const current = askRef.current;
		if (current && !threadHasUserQuestion(current.thread)) {
			setAsk(null);
			setAskError(null);
			return;
		}
		setAsk(null);
		setAskError(null);
	}, [stopAskStreaming]);

	const deleteAsk = useCallback(() => {
		stopAskStreaming();
		setAsk(null);
		setAskError(null);
	}, [stopAskStreaming]);

	return {
		ask,
		setAsk,
		streaming,
		askError,
		setAskError,
		resetAsk,
		sendAskQuestion,
		resendAskQuestion,
		hideAsk,
		deleteAsk,
		stopAskStreaming,
	};
}
