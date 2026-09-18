/**
 * Shared scaffolding for the Zotero dialogs (migrate + sync): the
 * auto-detect effect skeleton, the folder-pick flow, and remembered-options
 * loading. Dialog business logic (scan, migrate, sync) stays in each dialog.
 */

import { useEffect, useRef, useState } from "react";
import { readJsonStorage } from "@/lib/core/storage";
import { isTauri } from "@/lib/core/tauri";
import { pickZoteroDir } from "@/lib/paper/import/zotero-migrate";

/**
 * Auto-detect a Zotero library while the dialog is open.
 *
 * Runs `detect` inside the shared skeleton — cancelled flag, `detecting`
 * spinner state, and swallowed errors (no default library just means the
 * user picks the folder manually). `deps` re-triggers detection exactly like
 * inline effect closures (settings dir / remembered dir).
 */
export function useZoteroDirDetect(
	open: boolean,
	dir: string | null,
	options: {
		detect: (isCancelled: () => boolean) => Promise<void>;
		/** Extra values that re-trigger detection (settings / remembered dir). */
		deps?: readonly unknown[];
	},
): boolean {
	const { detect, deps = [] } = options;
	const [detecting, setDetecting] = useState(false);
	const detectRef = useRef(detect);
	detectRef.current = detect;

	useEffect(() => {
		if (!open || dir || !isTauri()) return;
		let cancelled = false;
		void (async () => {
			setDetecting(true);
			try {
				await detectRef.current(() => cancelled);
			} catch {
				// no default library — the user picks the folder manually
			} finally {
				if (!cancelled) setDetecting(false);
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [open, dir, ...deps]);

	return detecting;
}

/** Open the native Zotero folder picker; hand non-cancelled picks back. */
export async function chooseZoteroFolder(
	setError: (error: string | null) => void,
	onPicked: (dir: string) => void | Promise<void>,
): Promise<void> {
	setError(null);
	const picked = await pickZoteroDir();
	if (picked) await onPicked(picked);
}

/** Merge remembered dialog options (localStorage) over per-field defaults. */
export function loadStoredOpts<T extends object>(key: string, defaults: T): T {
	return { ...defaults, ...readJsonStorage<Partial<T>>(key, {}) };
}
