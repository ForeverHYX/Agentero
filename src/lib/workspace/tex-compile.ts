/**
 * LaTeX engine state and compile lifecycle for .tex files, kept in a lib-level
 * store so plain workspace actions (⌘\ split, tab buttons) can compile too —
 * not just the file-tree React hook. Mirrors the vaultStore vanilla-store
 * pattern; the hook stays a thin adapter.
 */

import { listen } from "@tauri-apps/api/event";
import { createStore } from "zustand/vanilla";
import i18n from "@/i18n";
import { isBackgroundTaskCancelledError } from "@/lib/core/background-tasks";
import { commands } from "@/lib/core/bindings";
import { notifyError, notifySuccess } from "@/lib/core/notify";
import { enqueueTaskSettled } from "@/lib/core/tasks";
import { vaultStore } from "@/lib/vault/store";
import { texPdfPath } from "@/lib/workspace/viewer";

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

let logDrained = false;
// Once the user explicitly picks an engine, never overwrite their choice
// on subsequent engine-list refreshes.
let userPickedEngine = false;
// Single-flight detection promise: callers can await it instead of racing
// the async result (compile right after a window reload used to read an
// empty list and falsely report "no engine").
let enginesPromise: Promise<void> | null = null;

/**
 * Detect engines (single-flight; concurrent callers share one invoke).
 * The promise is dropped when the scan fails or finds nothing, so the next
 * entry point retries instead of staying engine-less until a window reload.
 */
export function ensureTexEngines(): Promise<void> {
	if (!enginesPromise) {
		enginesPromise = (async () => {
			texCompileStore.setState({ enginesLoading: true });
			try {
				const res = await commands.detectLatexEngines();
				if (res.ok && res.data && res.data.length > 0) {
					const prev = texCompileStore.getState();
					texCompileStore.setState({ engines: res.data });
					// First-time default: only seed if the user has not picked yet.
					if (!prev.selectedEngine && !userPickedEngine) {
						texCompileStore.setState({ selectedEngine: res.data[0].id });
					}
				} else {
					// Failed scan or engine-less host: allow a later retry (a
					// TeX install mid-session is picked up by the next compile).
					enginesPromise = null;
				}
			} catch {
				// Transient IPC failure (e.g. during window reload): retry later.
				enginesPromise = null;
			} finally {
				texCompileStore.setState({ enginesLoading: false });
			}
		})();
	}
	return enginesPromise;
}

/**
 * Kick off engine detection (idempotent; safe to call from every entry
 * point). Also wires the compile:log drain. Await `ensureTexEngines` when
 * the result is needed.
 */
export function initTexEngines(): void {
	if (logDrained === false) {
		logDrained = true;
		listen<{ line: string }>("compile:log", () => {
			// Drain log events; log UI can be added later.
		});
	}
	void ensureTexEngines();
}

export function selectTexEngine(id: string): void {
	userPickedEngine = true;
	texCompileStore.setState({ selectedEngine: id });
}

/**
 * Clear the regenerable LaTeX intermediates (latexmk -c) for one source,
 * keeping the PDF. Escape hatch for latexmk's stuck state after a failed
 * run: its fingerprint database (`.fdb_latexmk`) records the error and,
 * with an unchanged source, it refuses to recompile ("Nothing to do …
 * gave an error in previous invocation"). Clearing the intermediates
 * resets that database so the next compile is a full run.
 */
export async function cleanTexAuxFiles(texPath: string): Promise<boolean> {
	// latexmk -c mid-compile would delete files the run is still writing.
	if (texCompileStore.getState().compilingPath) return false;
	try {
		const res = await commands.cleanLatexAuxFiles(texPath);
		if (!res.ok) {
			notifyError(
				res.error?.message || i18n.t("sidebar:fileTree.cleanAuxFailed"),
			);
			return false;
		}
		notifySuccess(i18n.t("sidebar:fileTree.cleanAuxSuccess"));
		return true;
	} catch (e) {
		notifyError(
			e instanceof Error && e.message
				? e.message
				: i18n.t("sidebar:fileTree.cleanAuxFailed"),
		);
		return false;
	}
}

/**
 * Compile with the selected (or first detected) engine. Runs as a background
 * job: the tasks panel shows live latexmk progress (rule / run milestones)
 * and a cancel button; `compilingPath` still drives the file-tree spinner.
 * Returns the absolute pdf path on success, else null.
 *
 * `quietSuccess` skips the success toast (save-triggered compiles would spam
 * one per autosave); failures always notify.
 */
export async function compileTexFile(
	texPath: string,
	opts?: { quietSuccess?: boolean },
): Promise<string | null> {
	initTexEngines();
	// Wait for the in-flight scan: reading the store immediately after a
	// window reload races detection and falsely reports "no engine".
	await ensureTexEngines();
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
		await enqueueTaskSettled({
			kind: "latexCompile",
			vaultPath: vaultStore.getState().vaultPath ?? "",
			path: texPath,
			lane: "focus",
			force: true,
			params: { engine },
		});
		if (!opts?.quietSuccess) {
			notifySuccess(i18n.t("sidebar:fileTree.compileSuccess"));
		}
		// latexmk writes {stem}.pdf next to the source (deterministic path).
		return texPdfPath(texPath);
	} catch (e) {
		if (!isBackgroundTaskCancelledError(e)) {
			notifyError(
				e instanceof Error && e.message
					? e.message
					: i18n.t("sidebar:fileTree.compileFailed"),
			);
		}
		return null;
	} finally {
		texCompileStore.setState({ compilingPath: null });
	}
}
