import { describe, expect, it } from "vitest";
import {
	getScrollSyncPartner,
	getScrollSyncPeer,
	getScrollSyncRole,
	mapScrollByContent,
	registerScrollSyncPair,
	registerScrollSyncPeer,
	type ScrollSyncPeer,
	type SyncScrollMetrics,
	unregisterScrollSyncPair,
} from "@/lib/pdf/scroll-sync";

function metrics(
	partial: Partial<SyncScrollMetrics> &
		Pick<SyncScrollMetrics, "scrollTop" | "scrollHeight" | "clientHeight">,
): SyncScrollMetrics {
	return {
		scrollLeft: 0,
		scrollWidth: 100,
		clientWidth: 100,
		...partial,
	};
}

function createPeer(initial: {
	scrollTop?: number;
	scrollHeight?: number;
	clientHeight?: number;
	zoom?: number;
}): ScrollSyncPeer & {
	scrollTop: number;
	zoom: number;
	emitScroll: () => void;
	emitZoom: () => void;
} {
	let scrollTop = initial.scrollTop ?? 0;
	const scrollHeight = initial.scrollHeight ?? 1000;
	const clientHeight = initial.clientHeight ?? 200;
	let zoom = initial.zoom ?? 1;
	const scrollListeners = new Set<() => void>();
	const zoomListeners = new Set<(next: number) => void>();
	return {
		get scrollTop() {
			return scrollTop;
		},
		get zoom() {
			return zoom;
		},
		getMetrics: () => metrics({ scrollTop, scrollHeight, clientHeight }),
		scrollTo: ({ y }) => {
			scrollTop = y;
		},
		onScrollChange: (listener) => {
			scrollListeners.add(listener);
			return () => scrollListeners.delete(listener);
		},
		getZoom: () => zoom,
		setZoom: (next) => {
			zoom = next;
		},
		onZoomChange: (listener) => {
			zoomListeners.add(listener);
			return () => zoomListeners.delete(listener);
		},
		emitScroll: () => {
			for (const listener of scrollListeners) listener();
		},
		emitZoom: () => {
			for (const listener of zoomListeners) listener(zoom);
		},
	};
}

describe("mapScrollByContent", () => {
	it("keeps both viewport centers on the same content point across differently sized panes", () => {
		// Same document in two panes with different viewport heights. The
		// source center sits on content y = 700; the target must aim its
		// center there too (700 - 150). The old ratio mapping landed the
		// target center on y = 675 instead.
		const mapped = mapScrollByContent(
			metrics({
				scrollTop: 600,
				scrollHeight: 1000,
				clientHeight: 200,
			}),
			metrics({
				scrollTop: 0,
				scrollHeight: 1000,
				clientHeight: 300,
			}),
		);
		expect(mapped).toEqual({ x: 0, y: 550 });
	});

	it("clamps to the target's scrollable range", () => {
		const mapped = mapScrollByContent(
			metrics({ scrollTop: 0, scrollHeight: 1000, clientHeight: 200 }),
			metrics({ scrollTop: 0, scrollHeight: 1000, clientHeight: 400 }),
		);
		// Source center is content y = 100; target top would be -100.
		expect(mapped).toEqual({ x: 0, y: 0 });
	});

	it("returns null while either viewport has no layout yet", () => {
		expect(
			mapScrollByContent(
				metrics({ scrollTop: 0, scrollHeight: 0, clientHeight: 200 }),
				metrics({ scrollTop: 0, scrollHeight: 1000, clientHeight: 200 }),
			),
		).toBeNull();
	});
});

describe("scroll sync registry", () => {
	it("pairs source and target and exposes roles", () => {
		const groupId = registerScrollSyncPair("paper-a", "paper-a::translation");
		expect(getScrollSyncPartner("paper-a")).toBe("paper-a::translation");
		expect(getScrollSyncPartner("paper-a::translation")).toBe("paper-a");
		expect(getScrollSyncRole("paper-a")).toBe("source");
		expect(getScrollSyncRole("paper-a::translation")).toBe("target");
		unregisterScrollSyncPair(groupId);
		expect(getScrollSyncPartner("paper-a")).toBeNull();
	});

	it("replaces a stale pair for the same documents", () => {
		registerScrollSyncPair("paper-a", "paper-a::translation");
		registerScrollSyncPair("paper-a", "paper-a::translation");
		expect(getScrollSyncPartner("paper-a")).toBe("paper-a::translation");
	});

	it("registers peers and clears them on dispose", () => {
		const peer = createPeer({});
		const dispose = registerScrollSyncPeer("paper-a", peer);
		expect(getScrollSyncPeer("paper-a")).toBe(peer);
		dispose();
		expect(getScrollSyncPeer("paper-a")).toBeNull();
	});
});
