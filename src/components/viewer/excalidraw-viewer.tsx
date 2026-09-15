import {
	Excalidraw,
	restoreAppState,
	restoreElements,
	serializeAsJSON,
} from "@excalidraw/excalidraw";
// Required base styles — without them the canvas and toolbars collapse.
import "@excalidraw/excalidraw/index.css";
import type {
	ExcalidrawInitialDataState,
	ExcalidrawProps,
} from "@excalidraw/excalidraw/types";
import { useTheme } from "next-themes";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

interface ExcalidrawViewerProps {
	seed: string;
	path: string;
	reloadKey: number;
	onPersist: (
		path: string,
		content: string,
		lastSaved: string,
	) => Promise<boolean>;
	onDirtyChange: (dirty: boolean) => void;
	className?: string;
}

const AUTOSAVE_DELAY_MS = 800;

const UI_OPTIONS: NonNullable<ExcalidrawProps["UIOptions"]> = {
	tools: { image: false },
};

export function ExcalidrawViewer({
	seed,
	path,
	reloadKey,
	onPersist,
	onDirtyChange,
	className,
}: ExcalidrawViewerProps) {
	const { i18n } = useTranslation("viewer");
	// Follow the app theme (system / light / dark preference via next-themes).
	const { resolvedTheme } = useTheme();
	const theme = resolvedTheme === "dark" ? "dark" : "light";
	const lastSavedRef = useRef(seed);
	const pendingJsonRef = useRef<string | null>(null);
	const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
	const [dirty, setDirty] = useState(false);

	// Keep the dirty callback stable across parent re-renders (doc-view passes
	// an inline arrow for `onDirtyChange`).
	const onDirtyChangeRef = useRef(onDirtyChange);
	onDirtyChangeRef.current = onDirtyChange;

	const initialData = useMemo<ExcalidrawInitialDataState>(() => {
		try {
			const parsed = JSON.parse(seed || "{}") as Record<string, unknown>;
			const elements = Array.isArray(parsed.elements) ? parsed.elements : [];
			const appState =
				typeof parsed.appState === "object" && parsed.appState != null
					? (parsed.appState as Record<string, unknown>)
					: {};
			return {
				elements: restoreElements(elements, null),
				appState: restoreAppState(appState, null),
			};
		} catch {
			return {
				elements: restoreElements([], null),
				appState: restoreAppState({}, null),
			};
		}
	}, [seed]);

	const flush = useCallback(async () => {
		const json = pendingJsonRef.current;
		if (json == null) return;
		pendingJsonRef.current = null;
		const ok = await onPersist(path, json, lastSavedRef.current);
		if (ok) {
			lastSavedRef.current = json;
			setDirty(false);
			onDirtyChangeRef.current(false);
		}
	}, [onPersist, path]);

	const schedulePersist = useCallback(
		(json: string) => {
			pendingJsonRef.current = json;
			if (debounceRef.current) clearTimeout(debounceRef.current);
			debounceRef.current = setTimeout(() => {
				debounceRef.current = null;
				void flush();
			}, AUTOSAVE_DELAY_MS);
		},
		[flush],
	);

	useEffect(() => {
		return () => {
			if (debounceRef.current) {
				clearTimeout(debounceRef.current);
				debounceRef.current = null;
			}
			if (pendingJsonRef.current) {
				void flush();
			}
		};
	}, [flush]);

	// The seed is authoritative: after our own save it equals what we wrote
	// (idempotent), after an external change it is the new disk snapshot.
	useEffect(() => {
		lastSavedRef.current = seed;
	}, [seed]);

	// A `reloadKey` bump means the canvas was reloaded from disk (external
	// change the user accepted). Any pending autosave from the superseded
	// editing session must be dropped, or its flush would pass the conflict
	// check (disk === lastSaved) and overwrite the reloaded content.
	useEffect(() => {
		if (reloadKey === 0) return; // mount: nothing to drop
		if (debounceRef.current) {
			clearTimeout(debounceRef.current);
			debounceRef.current = null;
		}
		pendingJsonRef.current = null;
		setDirty(false);
		onDirtyChangeRef.current(false);
	}, [reloadKey]);

	const handleChange: NonNullable<ExcalidrawProps["onChange"]> = useCallback(
		(elements, appState) => {
			if (!dirty) {
				setDirty(true);
				onDirtyChangeRef.current(true);
			}
			const payload = serializeAsJSON(elements, appState, {}, "local");
			schedulePersist(payload);
		},
		[dirty, schedulePersist],
	);

	// An empty or unreadable file still opens a blank canvas — the next
	// autosave creates/repairs the file on disk.
	return (
		<div className={`h-full w-full ${className ?? ""}`}>
			<Excalidraw
				key={reloadKey}
				initialData={initialData}
				onChange={handleChange}
				theme={theme}
				langCode={i18n.language}
				UIOptions={UI_OPTIONS}
			/>
		</div>
	);
}
