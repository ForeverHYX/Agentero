/**
 * Single subscriber hub for Tauri OS file-drag events (composer images,
 * Library PDFs, …). HTML5 DataTransfer is often empty for Finder / Preview
 * / other-app drops on macOS WKWebView; these events carry absolute paths.
 */
import type { UnlistenFn } from "@tauri-apps/api/event";
import type { DragDropEvent } from "@tauri-apps/api/webview";
import { isTauri } from "@/lib/core/tauri";

export type TauriFileDropPayload = DragDropEvent;

type Handler = (payload: TauriFileDropPayload) => unknown;

type HandlerRegistration = {
	handler: Handler;
	priority: number;
};

const handlers = new Set<HandlerRegistration>();
let startPromise: Promise<UnlistenFn | null> | null = null;

function dispatch(payload: TauriFileDropPayload): void {
	const ordered = [...handlers].sort((a, b) => b.priority - a.priority);
	for (const registration of ordered) {
		if (registration.handler(payload) === true) break;
	}
}

async function ensureStarted(): Promise<UnlistenFn | null> {
	if (!isTauri()) return null;
	if (startPromise) return startPromise;
	startPromise = (async () => {
		const { getCurrentWindow } = await import("@tauri-apps/api/window");
		return getCurrentWindow().onDragDropEvent((event) => {
			dispatch(event.payload);
		});
	})().catch((error) => {
		startPromise = null;
		console.warn("[agentero] tauri file-drop listen failed", error);
		return null;
	});
	return startPromise;
}

/**
 * Higher-priority handlers run first. Returning `true` claims the payload so
 * lower-priority fallbacks cannot import the same native drop twice.
 */
export function subscribeTauriFileDrop(
	handler: Handler,
	options?: { priority?: number },
): () => void {
	const registration = { handler, priority: options?.priority ?? 0 };
	handlers.add(registration);
	void ensureStarted();
	return () => {
		handlers.delete(registration);
	};
}

function pointInRect(x: number, y: number, rect: DOMRect): boolean {
	return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
}

/**
 * Tauri reports `PhysicalPosition`, but some macOS builds already match CSS
 * pixels. Try both spaces so a drop on the right-rail composer is not missed.
 */
export function isPhysicalPointInRect(
	position: {
		x: number;
		y: number;
		toLogical?: (factor: number) => { x: number; y: number };
	},
	rect: DOMRect,
): boolean {
	if (pointInRect(position.x, position.y, rect)) return true;
	const factor =
		typeof window === "undefined" ? 1 : window.devicePixelRatio || 1;
	if (
		factor !== 1 &&
		pointInRect(position.x / factor, position.y / factor, rect)
	) {
		return true;
	}
	if (typeof position.toLogical === "function") {
		const logical = position.toLogical(factor);
		if (pointInRect(logical.x, logical.y, rect)) return true;
	}
	return false;
}

export function isClientPointInRect(
	x: number,
	y: number,
	rect: DOMRect,
): boolean {
	return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
}

/**
 * Hit-test the first element matching `selector` in physical px (see
 * `isPhysicalPointInRect` for the logical-coordinate fallback).
 */
export function isPhysicalPointInSelector(
	position: Parameters<typeof isPhysicalPointInRect>[0],
	selector: string,
): boolean {
	const el = document.querySelector(selector);
	return (
		el instanceof HTMLElement &&
		isPhysicalPointInRect(position, el.getBoundingClientRect())
	);
}
