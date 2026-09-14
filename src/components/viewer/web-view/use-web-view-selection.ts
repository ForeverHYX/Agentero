/**
 * Bridge-message → selection toolbar for proxied web papers.
 *
 * The page inside the iframe is cross-origin by construction; its injected
 * bridge (`features/web/proxy.rs`) reports selections / shortcuts / external
 * links over postMessage, and this hook turns them into the same floating
 * chrome the PDF and plaza surfaces have: auto-copy + toolbar, ephemeral
 * streaming translate card, ⌘K quick chat, ⌘L add-to-chat. Nothing persists —
 * web papers have no marks/ sidecar (known limitation, docs/frontend/web-view).
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
	createSelectionAskThread,
	useSelectionAsk,
} from "@/components/selection/use-selection-ask";
import type { ScreenPoint } from "@/components/viewer/pdf/types";
import {
	bridgeSelectionBottomRight,
	bridgeSelectionScreen,
	parseWebBridgeMessage,
} from "@/components/viewer/web-view/bridge-message";
import {
	attachAgentRun,
	cancelAgentRun,
	disposeAgentRun,
	listAgents,
	runOnce,
} from "@/lib/agent";
import { registerSelectionQuickChat } from "@/lib/agent/selection-quick-chat";
import {
	pinActiveSelection,
	publishSelection,
} from "@/lib/agent/selection-store";
import { copyTextToClipboard } from "@/lib/core/clipboard";
import { errorText } from "@/lib/core/error";
import { notifyError } from "@/lib/core/notify";
import { openExternalUrl } from "@/lib/core/open-external";
import { createTranslateRecord } from "@/lib/pdf/translate";
import {
	evictAgentTranslateSessionId,
	getAgentTranslateSessionId,
	setAgentTranslateSessionId,
} from "@/lib/pdf/translate/agent-session-cache";
import type { PdfTranslateRecord } from "@/lib/pdf/translate/types";
import { buildPlazaAskPrompt } from "@/lib/plaza/ask-prompt";
import { loadSettings } from "@/lib/settings";
import { openSettingsWindow } from "@/lib/shell/settings-window";
import { openRightTab } from "@/lib/shell/ui-window-actions";
import {
	buildTranslatePrompt,
	displayTranslateError,
	prepareTranslateTask,
	resolveTranslateAgent,
	runTranslate,
} from "@/lib/translate";
import { getVaultPath } from "@/lib/vault/store";
import { webViewProxyOrigins } from "@/lib/web-view/proxy-url";

const COPIED_LABEL_DURATION_MS = 1000;

export type WebViewSelectionMenu = {
	text: string;
	screen: ScreenPoint;
	/** Selection bottom-right corner — where the translate card opens. */
	bottomRight: ScreenPoint;
};

/** Message the app sends into the frame (`source` marker the bridge checks). */
function postToFrame(
	iframe: HTMLIFrameElement | null,
	type: "clearSelection" | "copySelection",
) {
	iframe?.contentWindow?.postMessage(
		{ source: "agentero-web-host", type },
		"*",
	);
}

export function useWebViewSelection({
	srcUrl,
	iframeRef,
	title,
}: {
	/** The real remote URL (also the ask/translate provenance). */
	srcUrl: string;
	iframeRef: RefObject<HTMLIFrameElement | null>;
	title?: string;
}) {
	const { t } = useTranslation("viewer");
	const [menu, setMenu] = useState<WebViewSelectionMenu | null>(null);
	const [copiedLabelPos, setCopiedLabelPos] = useState<ScreenPoint | null>(
		null,
	);
	const labelTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

	// Ephemeral single translate card (no marks/ to write into).
	const [translateRec, setTranslateRec] = useState<PdfTranslateRecord | null>(
		null,
	);
	const [translateScreen, setTranslateScreen] = useState<ScreenPoint | null>(
		null,
	);
	const [translateStreaming, setTranslateStreaming] = useState(false);
	const [translateError, setTranslateError] = useState<string | null>(null);
	const translateSessionRef = useRef<string | null>(null);
	const translateUnsubsRef = useRef<UnlistenFn[] | null>(null);
	const translateDisposedRef = useRef(false);
	const translateStreamingRef = useRef(false);
	const translateRecRef = useRef<PdfTranslateRecord | null>(null);
	translateRecRef.current = translateRec;

	// Viewer-wide single-run slot shared by the ask + translate clusters.
	const activeSessionRef = useRef<string | null>(null);

	const askCtl = useSelectionAsk<ScreenPoint>({
		buildPrompt: (thread, question) =>
			buildPlazaAskPrompt(thread, question, {
				title,
				url: srcUrl,
				surface: "web",
			}),
		activeSessionRef,
	});
	const { ask, streaming, askError, resetAsk, setAsk } = askCtl;

	const clearCopiedLabel = useCallback(() => {
		if (labelTimerRef.current) {
			clearTimeout(labelTimerRef.current);
			labelTimerRef.current = null;
		}
		setCopiedLabelPos(null);
	}, []);

	const closeMenu = useCallback(() => {
		setMenu(null);
		clearCopiedLabel();
	}, [clearCopiedLabel]);

	// New page → drop selection chrome (covers in-frame navigations too:
	// the reload re-runs the bridge but not this React tree).
	// biome-ignore lint/correctness/useExhaustiveDependencies: re-run on page navigation
	useEffect(() => {
		setMenu(null);
		resetAsk();
		setTranslateRec(null);
		setTranslateScreen(null);
		setTranslateError(null);
		clearCopiedLabel();
	}, [srcUrl, resetAsk, clearCopiedLabel]);

	// Terminal teardown for the translate run (mirrors the ask cluster).
	useEffect(() => {
		translateDisposedRef.current = false;
		return () => {
			disposeAgentRun({
				disposedRef: translateDisposedRef,
				unsubsRef: translateUnsubsRef,
				sessionRef: translateSessionRef,
				activeSessionRef,
			});
			translateStreamingRef.current = false;
		};
	}, []);

	const stopTranslateSession = useCallback(() => {
		const sid = translateSessionRef.current;
		if (sid) {
			void cancelAgentRun(sid).catch(() => undefined);
			if (activeSessionRef.current === sid) activeSessionRef.current = null;
			translateSessionRef.current = null;
		}
		translateStreamingRef.current = false;
		setTranslateStreaming(false);
	}, []);

	const handleAddToChat = useCallback(() => {
		if (!menu) return;
		const text = menu.text;
		setMenu(null);
		clearCopiedLabel();
		// Focus sits in the frame; ask its bridge to clear the selection.
		postToFrame(iframeRef.current, "clearSelection");
		publishSelection({
			text,
			sourcePath: srcUrl,
			origin: "markdown",
		});
		pinActiveSelection();
		openRightTab("agent");
	}, [menu, srcUrl, iframeRef, clearCopiedLabel]);

	const handleAsk = useCallback(() => {
		if (!menu) return;
		const { text, screen } = menu;
		setMenu(null);
		clearCopiedLabel();
		setAsk({
			thread: createSelectionAskThread(srcUrl, text),
			screen,
		});
	}, [menu, srcUrl, setAsk, clearCopiedLabel]);

	const handleTranslate = useCallback(() => {
		if (!menu) return;
		const quote = menu.text;
		// Anchor below-right of the selection: SelectionCard opens to the right
		// of (or right-aligned under) the anchor, and `trackPin` places its top
		// 8px above the anchor — +8 lands it right under the selected text.
		const screen = {
			x: menu.bottomRight.x,
			y: menu.bottomRight.y + 8,
		};
		setMenu(null);
		clearCopiedLabel();

		stopTranslateSession();
		const rec = createTranslateRecord({
			paperPath: srcUrl,
			page: 1,
			rects: [],
			quote,
		});
		setTranslateRec(rec);
		setTranslateScreen(screen);
		setTranslateStreaming(true);
		translateStreamingRef.current = true;
		setTranslateError(null);

		const { providerId, targetLangName } = prepareTranslateTask({
			text: quote,
			context: { surface: "web-selection" },
		});

		if (providerId === "agent") {
			const prompt = buildTranslatePrompt({
				text: quote,
				targetLangName,
				surface: "web-selection",
			});
			void (async () => {
				try {
					const registry = await listAgents().catch(() => null);
					const resolved = resolveTranslateAgent(
						loadSettings().translate,
						registry,
					);
					if (!resolved.agentId) {
						const msg = t("selection.translateNoAgent");
						notifyError(msg);
						setTranslateRec((prev) =>
							prev && prev.id === rec.id
								? { ...prev, error: msg, updatedAt: new Date().toISOString() }
								: prev,
						);
						return;
					}
					const agentId = resolved.agentId;
					const modelId = resolved.modelId;
					const accepted = await runOnce({
						prompt,
						agentId,
						modelId,
						sessionId:
							getAgentTranslateSessionId(srcUrl, agentId, modelId) ?? undefined,
						vaultPath: getVaultPath() ?? undefined,
						workflow: "translate",
						permissionMode: "auto",
						hideFromChatHistory: true,
					});
					await attachAgentRun({
						accepted,
						disposedRef: translateDisposedRef,
						unsubsRef: translateUnsubsRef,
						sessionRef: translateSessionRef,
						activeSessionRef,
						onStream: (ev) => {
							const latest = translateRecRef.current;
							if (latest?.id !== rec.id) return;
							setTranslateRec({
								...latest,
								result: (latest.result ?? "") + ev.chunk,
								updatedAt: new Date().toISOString(),
								error: undefined,
							});
						},
						onCompleted: (ev) => {
							const latest = translateRecRef.current;
							if (latest?.id !== rec.id) return;
							setTranslateRec({
								...latest,
								result: (ev.content || latest.result || "").trim(),
								updatedAt: new Date().toISOString(),
								error: undefined,
							});
							setTranslateError(null);
							if (ev.providerSessionId && ev.stopReason !== "cancelled") {
								setAgentTranslateSessionId(
									srcUrl,
									agentId,
									modelId,
									ev.providerSessionId,
								);
							}
						},
						onFailed: (ev) => {
							evictAgentTranslateSessionId(srcUrl, agentId, modelId);
							const msg = ev.error || t("pdfAsk.agentFailed");
							notifyError(msg);
							setTranslateRec((prev) =>
								prev && prev.id === rec.id
									? { ...prev, error: msg, updatedAt: new Date().toISOString() }
									: prev,
							);
						},
						onSettled: () => {
							translateStreamingRef.current = false;
							setTranslateStreaming(false);
						},
					});
				} catch (e) {
					const message = errorText(e);
					notifyError(message);
					setTranslateRec((prev) =>
						prev && prev.id === rec.id
							? { ...prev, error: message, updatedAt: new Date().toISOString() }
							: prev,
					);
					translateStreamingRef.current = false;
					setTranslateStreaming(false);
				}
			})();
			return;
		}

		void (async () => {
			try {
				const result = await runTranslate(
					{ text: quote, context: { surface: "web-selection" } },
					{ providerId },
				);
				const latest = translateRecRef.current;
				if (latest?.id !== rec.id) return;
				setTranslateRec({
					...latest,
					result: result.trim(),
					updatedAt: new Date().toISOString(),
					error: undefined,
				});
				translateStreamingRef.current = false;
				setTranslateStreaming(false);
				setTranslateError(null);
			} catch (e) {
				const message = displayTranslateError(errorText(e));
				notifyError(message);
				setTranslateRec((prev) =>
					prev && prev.id === rec.id
						? { ...prev, error: message, updatedAt: new Date().toISOString() }
						: prev,
				);
				translateStreamingRef.current = false;
				setTranslateStreaming(false);
			}
		})();
	}, [menu, srcUrl, t, stopTranslateSession, clearCopiedLabel]);

	const hideTranslate = useCallback(() => {
		stopTranslateSession();
		setTranslateRec(null);
		setTranslateScreen(null);
		setTranslateError(null);
	}, [stopTranslateSession]);

	// ⌘K Quick chat — while this web selection toolbar is armed. ⌘L arrives
	// from the bridge (keyboard focus never leaves the frame).
	const menuRef = useRef(menu);
	menuRef.current = menu;
	const handleAskRef = useRef(handleAsk);
	handleAskRef.current = handleAsk;
	const handleAddToChatRef = useRef(handleAddToChat);
	handleAddToChatRef.current = handleAddToChat;
	useEffect(() => {
		return registerSelectionQuickChat(() => {
			if (!menuRef.current) return false;
			handleAskRef.current();
			return true;
		});
	}, []);

	// Bridge messages: selections position the toolbar, scroll hides it,
	// shortcuts trigger the actions, external links go to the system browser.
	useEffect(() => {
		const origins = webViewProxyOrigins();
		const onMessage = (event: MessageEvent) => {
			if (!origins.includes(event.origin)) return;
			const msg = parseWebBridgeMessage(event.data);
			if (!msg) return;
			switch (msg.type) {
				case "selection": {
					const iframe = iframeRef.current;
					if (!iframe) return;
					if (!msg.text || !msg.rect) {
						setMenu(null);
						return;
					}
					const ir = iframe.getBoundingClientRect();
					const screen = bridgeSelectionScreen(msg.rect, ir);
					setMenu({
						text: msg.text,
						screen,
						bottomRight: bridgeSelectionBottomRight(msg.rect, ir),
					});
					// The bridge already execCommand-copied inside the frame; the
					// app-side write can be refused while focus sits there — try
					// anyway so the label only shows on a real copy.
					void copyTextToClipboard(msg.text);
					clearCopiedLabel();
					setCopiedLabelPos(screen);
					labelTimerRef.current = setTimeout(() => {
						labelTimerRef.current = null;
						setCopiedLabelPos(null);
					}, COPIED_LABEL_DURATION_MS);
					return;
				}
				case "scroll":
					setMenu(null);
					return;
				case "shortcut":
					if (msg.id === "quickChat") handleAskRef.current();
					else handleAddToChatRef.current();
					return;
				case "external":
					openExternalUrl(msg.url);
					return;
			}
		};
		window.addEventListener("message", onMessage);
		return () => window.removeEventListener("message", onMessage);
	}, [iframeRef, clearCopiedLabel]);

	return {
		menu,
		ask,
		streaming,
		askError,
		translateRec,
		translateScreen,
		translateStreaming,
		translateError,
		copiedLabelPos,
		closeMenu,
		handleAsk,
		handleAddToChat,
		handleTranslate,
		sendAskQuestion: askCtl.sendAskQuestion,
		resendAskQuestion: askCtl.resendAskQuestion,
		hideAsk: askCtl.hideAsk,
		deleteAsk: askCtl.deleteAsk,
		stopAskStreaming: askCtl.stopAskStreaming,
		hideTranslate,
		deleteTranslateCard: hideTranslate,
		openTranslateSettings: () => openSettingsWindow("translate"),
	};
}
