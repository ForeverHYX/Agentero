import { ArrowUp, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { useStore } from "zustand";
import { Button } from "@/components/ui/button";
import {
	beginSelectionComment,
	confirmSelectionChat,
	dismissSelectionChat,
	openSelectionChat,
	type SelectionChatDraft,
	selectionChatStore,
} from "@/lib/agent/selection-chat-store";
import { openRightTab } from "@/lib/shell/ui-window-actions";

const SURFACE = "[data-selection-chat-source]";

function surfaceOf(node: Node | null): HTMLElement | null {
	const element = node instanceof Element ? node : node?.parentElement;
	return element?.closest<HTMLElement>(SURFACE) ?? null;
}

/** One host per window: survives PDF selection teardown and portalled editors. */
export function SelectionChatPopover() {
	const draft = useStore(selectionChatStore, (state) => state.draft);
	const popoverRef = useRef<HTMLDivElement>(null);
	useEffect(() => {
		let frame = 0;
		const capture = () => {
			const selection = window.getSelection();
			if (!selection || selection.isCollapsed || !selection.rangeCount) return;
			const source = surfaceOf(selection.anchorNode);
			if (!source || source !== surfaceOf(selection.focusNode)) return;
			// Source editors may contain nested form controls; never quote their drafts.
			if (document.activeElement?.matches("input, textarea")) return;
			const origin = source.dataset.selectionChatOrigin;
			if (origin !== "markdown" && origin !== "chat") return;
			const range = selection.getRangeAt(0);
			const rect = range.getBoundingClientRect();
			if (!rect.width && !rect.height) return;
			openSelectionChat(
				{
					text: selection.toString(),
					sourcePath: source.dataset.selectionChatSource ?? "",
					origin,
					messageId: source.dataset.selectionChatMessage,
				},
				{ x: rect.left + rect.width / 2, y: rect.top },
				"menu",
			);
		};
		const scheduleCapture = (event: Event) => {
			if (event instanceof PointerEvent && event.button !== 0) return;
			if (
				event.target instanceof Node &&
				popoverRef.current?.contains(event.target)
			)
				return;
			if (
				event instanceof KeyboardEvent &&
				(event.key === "Escape" ||
					event.key === "Enter" ||
					event.metaKey ||
					event.ctrlKey)
			)
				return;
			cancelAnimationFrame(frame);
			frame = requestAnimationFrame(capture);
		};
		const onDown = (event: PointerEvent) => {
			if (
				event.target instanceof Node &&
				popoverRef.current?.contains(event.target)
			)
				return;
			dismissSelectionChat();
		};
		const onScroll = (event: Event) => {
			if (
				event.target instanceof Node &&
				popoverRef.current?.contains(event.target)
			)
				return;
			if (selectionChatStore.getState().draft?.stage === "menu")
				dismissSelectionChat();
		};
		const onSelectionChange = () => {
			if (
				selectionChatStore.getState().draft?.stage === "menu" &&
				window.getSelection()?.isCollapsed
			)
				dismissSelectionChat();
		};
		document.addEventListener("pointerdown", onDown, true);
		document.addEventListener("pointerup", scheduleCapture);
		document.addEventListener("keyup", scheduleCapture);
		document.addEventListener("selectionchange", onSelectionChange);
		window.addEventListener("scroll", onScroll, true);
		return () => {
			cancelAnimationFrame(frame);
			document.removeEventListener("pointerdown", onDown, true);
			document.removeEventListener("pointerup", scheduleCapture);
			document.removeEventListener("keyup", scheduleCapture);
			document.removeEventListener("selectionchange", onSelectionChange);
			window.removeEventListener("scroll", onScroll, true);
		};
	}, []);
	if (!draft) return null;
	return createPortal(
		<div ref={popoverRef}>
			<SelectionChatCard key={draft.selection.id} draft={draft} />
		</div>,
		document.body,
	);
}

function SelectionChatCard({ draft }: { draft: SelectionChatDraft }) {
	const { t } = useTranslation(["viewer", "common"]);
	const [comment, setComment] = useState("");
	const inputRef = useRef<HTMLTextAreaElement>(null);
	const priorFocus = useRef(document.activeElement);
	const [viewport, setViewport] = useState({
		width: window.innerWidth,
		height: window.innerHeight,
	});
	useEffect(() => {
		const resize = () =>
			setViewport({ width: window.innerWidth, height: window.innerHeight });
		window.addEventListener("resize", resize);
		return () => window.removeEventListener("resize", resize);
	}, []);
	useEffect(() => {
		if (draft.stage === "comment")
			inputRef.current?.focus({ preventScroll: true });
	}, [draft.stage]);
	useEffect(() => {
		const onEscape = (event: KeyboardEvent) => {
			if (event.key !== "Escape" || event.isComposing) return;
			event.preventDefault();
			event.stopPropagation();
			dismissSelectionChat();
			if (
				priorFocus.current instanceof HTMLElement &&
				priorFocus.current.isConnected
			)
				priorFocus.current.focus({ preventScroll: true });
		};
		document.addEventListener("keydown", onEscape, true);
		return () => document.removeEventListener("keydown", onEscape, true);
	}, []);
	const isComment = draft.stage === "comment";
	const width = Math.min(isComment ? 360 : 144, viewport.width - 24);
	const height = isComment ? 152 : 36;
	const left = Math.max(
		12,
		Math.min(draft.screen.x - width / 2, viewport.width - width - 12),
	);
	const preferredTop = draft.screen.y - height - 10;
	const top = Math.max(
		12,
		Math.min(
			preferredTop >= 12 ? preferredTop : draft.screen.y + 24,
			viewport.height - height - 12,
		),
	);
	const confirm = () => {
		if (confirmSelectionChat(comment)) openRightTab("agent");
	};
	return (
		<div
			role="dialog"
			aria-label={t("selection.addToChat")}
			className="fixed z-50 rounded-xl border border-border bg-popover text-popover-foreground shadow-lg"
			style={{ left, top, width }}
		>
			{isComment ? (
				<form
					className="flex flex-col gap-2 p-3"
					onSubmit={(event) => {
						event.preventDefault();
						confirm();
					}}
				>
					<blockquote
						className="truncate border-l-2 border-primary/40 pl-2 text-xs text-muted-foreground"
						title={draft.selection.text}
					>
						{draft.selection.text}
					</blockquote>
					<textarea
						ref={inputRef}
						rows={2}
						aria-label={t("selection.chatCommentPlaceholder")}
						placeholder={t("selection.chatCommentPlaceholder")}
						className="w-full resize-none rounded-md bg-transparent px-1 py-1 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
						value={comment}
						onChange={(event) => setComment(event.target.value)}
						onKeyDown={(event) => {
							event.stopPropagation();
							if (
								event.key === "Enter" &&
								!event.shiftKey &&
								!event.nativeEvent.isComposing &&
								event.keyCode !== 229
							) {
								event.preventDefault();
								confirm();
							}
						}}
					/>
					<div className="flex justify-end gap-1">
						<Button
							type="button"
							variant="ghost"
							size="icon-sm"
							aria-label={t("common:cancel")}
							onClick={dismissSelectionChat}
						>
							<X className="size-4" />
						</Button>
						<Button
							type="submit"
							size="icon-sm"
							aria-label={t("selection.addToChat")}
						>
							<ArrowUp className="size-4" />
						</Button>
					</div>
				</form>
			) : (
				<button
					type="button"
					className="h-9 w-full cursor-pointer rounded-xl px-3 text-sm font-medium hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
					onPointerDown={(event) => event.preventDefault()}
					onClick={beginSelectionComment}
				>
					{t("selection.addToChat")}
				</button>
			)}
		</div>
	);
}
