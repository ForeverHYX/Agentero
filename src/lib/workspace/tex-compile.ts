/**
 * LaTeX engine state and compile lifecycle for .tex files, kept in a lib-level
 * store so plain workspace actions (⌘\ split, tab buttons) can compile too —
 * not just the file-tree React hook. Mirrors the vaultStore vanilla-store
 * pattern; the hook stays a thin adapter.
 */

import { listen } from "@tauri-apps/api/event";
import { createStore } from "zustand/vanilla";
import i18n from "@/i18n";
import { commands } from "@/lib/core/bindings";
import { notifyError, notifySuccess } from "@/lib/core/notify";

export type LatexEngine = {
	id: string;
	label: string;
	path: string | null;
};

type TexCompileState = {
	engines: LatexEngine[];
	enginesLoading: boolean;
	selectedEngine: string | null;
	compilingPath: string | null;
};

export const texCompileStore = createStore<TexCompileState>(() => ({
	engines: [],
	enginesLoading: false,
	selectedEngine: null,
	compilingPath: null,
}));

let enginesInited = false;
let logDrained = false;
// Once the user explicitly picks an engine, never overwrite their choice
// on subsequent engine-list refreshes.
let userPickedEngine = false;

/**
 * Detect engines once per window (idempotent; safe to call from every entry
 * point). Also wires the compile:log drain.
 */
export function initTexEngines(): void {
	if (logDrained === false) {
		logDrained = true;
		listen<{ line: string }>("compile:log", () => {
			// Drain log events; log UI can be added later.
		});
	}
	if (enginesInited) return;
	enginesInited = true;

	texCompileStore.setState({ enginesLoading: true });
	commands
		.detectLatexEngines()
		.then((res) => {
			if (res.ok && res.data) {
				const prev = texCompileStore.getState();
				texCompileStore.setState({ engines: res.data });
				// First-time default: only seed if the user has not picked yet.
				if (res.data.length > 0 && !prev.selectedEngine && !userPickedEngine) {
					texCompileStore.setState({ selectedEngine: res.data[0].id });
				}
			}
		})
		.catch(() => {
			// Silently ignore — button won't show if no engines found.
		})
		.finally(() => {
			texCompileStore.setState({ enginesLoading: false });
		});
}

export function selectTexEngine(id: string): void {
	userPickedEngine = true;
	texCompileStore.setState({ selectedEngine: id });
}

/**
 * Compile with the selected (or first detected) engine. Drives
 * `compilingPath` for the file-tree spinner and notifies success/failure.
 * Returns the absolute pdf path on success, else null.
 */
export async function compileTexFile(texPath: string): Promise<string | null> {
	initTexEngines();
	const { selectedEngine, engines, compilingPath } = texCompileStore.getState();
	const engine = selectedEngine ?? engines[0]?.id ?? null;
	if (!engine) {
		notifyError(i18n.t("sidebar:fileTree.selectEngineFirst"));
		return null;
	}
	// One in-flight compile at a time (prevents ⌘\ double-fire).
	if (compilingPath) return null;

	texCompileStore.setState({ compilingPath: texPath });
	try {
		const res = await commands.compileTex(texPath, engine);

		if (!res.ok) {
			notifyError(
				res.error?.message ?? i18n.t("sidebar:fileTree.compileFailed"),
			);
			return null;
		}

		const result = res.data;
		if (result?.ok && result.pdfPath) {
			notifySuccess(i18n.t("sidebar:fileTree.compileSuccess"));
			return result.pdfPath;
		}
		notifyError(i18n.t("sidebar:fileTree.compileFailed"));
		return null;
	} catch (e) {
		notifyError(String(e));
		return null;
	} finally {
		texCompileStore.setState({ compilingPath: null });
	}
}
