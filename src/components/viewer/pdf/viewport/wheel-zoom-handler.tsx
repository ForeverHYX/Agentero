import {
	useViewportCapability,
	useViewportElement,
} from "@embedpdf/plugin-viewport/react";
import { useZoom } from "@embedpdf/plugin-zoom/react";
import { useEffect, useLayoutEffect, useRef } from "react";
import { EMBED_PAGE_ATTR } from "@/components/viewer/pdf/coords";
import {
	bindZoomGesture,
	computeCenteredScrollLeft,
} from "@/lib/pdf/wheel-zoom";
import { clampZoomPreviewScale, zoomPreviewTranslate } from "@/lib/pdf/zoom";

/** Safety net for a dropped `gestureend`: commit the pending preview. */
const ZOOM_GESTURE_WATCHDOG_MS = 1200;

/** Zoom deltas below this are not worth a real relayout. */
const ZOOM_COMMIT_EPSILON = 1e-3;

/**
 * Ctrl/Cmd+wheel and trackpad pinch zoom.
 *
 * EmbedPDF's ZoomGestureWrapper maps a wheel tick to a scale factor of
 * `1 - deltaY * 0.01`, which collapses the zoom to its minimum on a single
 * mouse notch. We bind the gestures ourselves and drive the zoom the way that
 * wrapper drives touch: scale the gesture element with a CSS transform while
 * the fingers move, then commit one real zoom on release.
 *
 * Committing once is not just cheaper. Every real zoom re-lays out the scroller
 * and queues a viewport scroll request for the next frame; committing one per
 * animation frame (or per coarse fixed step) supersedes that anchor and walks
 * the viewport towards the start of the document, and a fixed step of 10–20%
 * made a slow pinch feel like it was not responding at all.
 *
 * Both the preview transform and the committed zoom anchor on the real
 * reading-area center, never on the pointer, so the post-commit horizontal
 * recenter agrees with the preview and the page does not shift on release.
 */
export function WheelZoomHandler({ docId }: { docId: string }) {
	const viewportRef = useViewportElement();
	const { provides: viewportCapability } = useViewportCapability();
	const viewportCapabilityRef = useRef(viewportCapability);
	viewportCapabilityRef.current = viewportCapability;
	const { provides: zoom, state: zoomState } = useZoom(docId);
	const zoomRef = useRef(zoom);
	zoomRef.current = zoom;
	const zoomLevelRef = useRef(1);
	zoomLevelRef.current = zoomState.currentZoomLevel || 1;
	/** EmbedPDF's ZoomGestureWrapper div: the node the preview transform scales. */
	const previewElementRef = useRef<HTMLElement | null>(null);
	/** Set while a committed zoom is waiting for its layout to land. */
	const pendingCommitRef = useRef(false);
	/** Scroll the zoom plugin derived for the commit, read in the same task. */
	const pendingScrollRef = useRef<{ left: number; top: number } | null>(null);

	// The scroller resizes with the zoom inside the same React commit as this
	// effect, before the browser paints. Applying the scroll the zoom plugin
	// derived and dropping the preview transform here keeps the release to a
	// single paint: the DOM never shows the new layout scaled a second time by
	// the preview transform (which, over a document-height element, stalls the
	// compositor), and the viewport's own deferred scroll arrives afterwards as a
	// no-op.
	useLayoutEffect(() => {
		if (
			!pendingCommitRef.current ||
			!Number.isFinite(zoomState.currentZoomLevel)
		)
			return;
		pendingCommitRef.current = false;
		const scroll = pendingScrollRef.current;
		pendingScrollRef.current = null;
		const element = previewElementRef.current;
		if (element) {
			element.style.transform = "";
			element.style.willChange = "";
		}
		const container = viewportRef?.current;
		if (!container || !scroll) return;
		if (Number.isFinite(scroll.left)) container.scrollLeft = scroll.left;
		if (Number.isFinite(scroll.top)) container.scrollTop = scroll.top;

		// EmbedPDF anchored the zoom on the comment-rail-narrowed clientWidth it
		// observes (DockviewViewport reserves the rail via rightGutter), so the
		// page sits ~rightGutter/2 left of the real screen center. This runs in the
		// same layout effect, after the scroller is laid out and before paint, so
		// the live page rects already reflect the committed zoom — pick the page
		// nearest the viewport's vertical center and recenter it horizontally.
		// Only the horizontal offset is corrected; scrollTop is preserved.
		if (container.scrollWidth > container.clientWidth) {
			const viewportRect = container.getBoundingClientRect();
			const viewportMidY = viewportRect.top + container.clientHeight / 2;
			let best: HTMLElement | null = null;
			let bestDist = Number.POSITIVE_INFINITY;
			for (const el of container.querySelectorAll<HTMLElement>(
				`[${EMBED_PAGE_ATTR}]`,
			)) {
				const r = el.getBoundingClientRect();
				const dist = Math.abs(r.top + r.height / 2 - viewportMidY);
				if (dist < bestDist) {
					bestDist = dist;
					best = el;
				}
			}
			if (best) {
				const pageRect = best.getBoundingClientRect();
				const pageCenter = pageRect.left + pageRect.width / 2;
				const viewportCenter = viewportRect.left + container.clientWidth / 2;
				container.scrollLeft = computeCenteredScrollLeft({
					scrollLeft: container.scrollLeft,
					scrollWidth: container.scrollWidth,
					clientWidth: container.clientWidth,
					pageCenter,
					viewportCenter,
				});
			}
		}
	}, [viewportRef, zoomState.currentZoomLevel]);

	useEffect(() => {
		const container = viewportRef?.current;
		if (!container) return;
		// Its own touch gestures transform this node, so the preview scales the same
		// subtree the zoom plugin would scale.
		previewElementRef.current =
			container.firstElementChild as HTMLElement | null;

		const resetPreview = () => {
			const element = previewElementRef.current;
			if (!element) return;
			element.style.transform = "";
			element.style.willChange = "";
		};

		let previewZoom = 1;
		let previewScale = 1;
		/** Fixed reading-area center captured at gesture start; the single anchor for preview, commit and recenter. */
		let anchor = { x: 0, y: 0 };
		/** The anchor in the transformed element's own coordinates. */
		let local = { x: 0, y: 0 };
		let watchdog: ReturnType<typeof setTimeout> | null = null;
		let running = false;

		const clearWatchdog = () => {
			if (watchdog === null) return;
			clearTimeout(watchdog);
			watchdog = null;
		};

		const commit = () => {
			if (!running) return;
			running = false;
			clearWatchdog();
			const containerRect = container.getBoundingClientRect();
			// Derived from the same fixed anchor as the preview transform, so the
			// post-commit layout effect and recenter never disagree with the preview.
			const focus = {
				vx: anchor.x - containerRect.left,
				vy: anchor.y - containerRect.top,
			};
			const target = previewZoom * previewScale;
			if (Math.abs(target - zoomLevelRef.current) < ZOOM_COMMIT_EPSILON) {
				resetPreview();
				return;
			}
			pendingCommitRef.current = true;
			zoomRef.current?.requestZoom(target, focus);
			// Read the scroll the plugin derived for this focus in the same task as
			// the request, before anything can walk the cached metrics back to the
			// DOM's (still pre-zoom) position.
			const metrics = viewportCapabilityRef.current
				?.forDocument(docId)
				.getMetrics();
			pendingScrollRef.current = metrics
				? { left: metrics.scrollLeft, top: metrics.scrollTop }
				: null;
			// Safety net: if the plugin snaps the request to its own grid and nothing
			// changes, no commit follows and the layout effect never runs.
			requestAnimationFrame(() => {
				if (!pendingCommitRef.current) return;
				pendingCommitRef.current = false;
				pendingScrollRef.current = null;
				resetPreview();
			});
		};

		const armWatchdog = () => {
			clearWatchdog();
			watchdog = setTimeout(commit, ZOOM_GESTURE_WATCHDOG_MS);
		};

		const binding = bindZoomGesture({
			target: container,
			onZoomStart: () => {
				if (running) commit();
				clearWatchdog();
				// Measure the element without a stale preview transform; a commit whose
				// layout effect has not run yet finishes through the plugin's own
				// deferred scroll instead.
				pendingCommitRef.current = false;
				resetPreview();
				// Ctrl/Cmd+wheel and trackpad pinch always anchor on the real
				// reading-area center, never the pointer position; preview, commit
				// focus and post-commit recenter share this one fixed point.
				const containerRect = container.getBoundingClientRect();
				anchor = {
					x: containerRect.left + container.clientWidth / 2,
					y: containerRect.top + container.clientHeight / 2,
				};
				previewZoom = zoomLevelRef.current || 1;
				previewScale = 1;
				running = true;
				const element = previewElementRef.current;
				if (element) {
					const elementRect = element.getBoundingClientRect();
					local = {
						x: anchor.x - elementRect.left,
						y: anchor.y - elementRect.top,
					};
					element.style.transformOrigin = "0 0";
					// Rasterize the pages once and let the compositor scale that raster
					// for the rest of the gesture. Without it the compositor re-rasterizes
					// on every scale change, which shows up as jank when a pinch turns
					// around (zoom in, then straight back out). Cleared with the transform
					// so it can never outlive the gesture.
					element.style.willChange = "transform";
				}
				armWatchdog();
			},
			onZoomChange: (ratio) => {
				if (!running) return;
				previewScale = clampZoomPreviewScale(ratio, previewZoom);
				armWatchdog();
				const element = previewElementRef.current;
				if (!element) return;
				const offset = zoomPreviewTranslate(local.x, local.y, previewScale);
				element.style.transform = `translate(${offset.x}px, ${offset.y}px) scale(${previewScale})`;
			},
			onZoomEnd: commit,
		});

		return () => {
			binding.dispose();
			clearWatchdog();
			pendingCommitRef.current = false;
			resetPreview();
		};
	}, [docId, viewportRef]);

	return null;
}
