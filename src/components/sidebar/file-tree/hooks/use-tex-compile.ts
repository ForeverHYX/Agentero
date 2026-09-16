/**
 * Compile state and actions for .tex files in the file tree.
 * Manages engine detection, busy state, and compilation lifecycle.
 */

import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { commands } from "@/lib/core/bindings";
import { notifyError, notifySuccess } from "@/lib/core/notify";
import { openTab } from "@/lib/workspace/actions";
import type { CenterViewMode } from "@/lib/workspace/viewer";

export type LatexEngine = {
	id: string;
	label: string;
	path: string | null;
};

export type TexCompileActions = {
	engines: LatexEngine[];
	enginesLoading: boolean;
	selectedEngine: string | null;
	selectEngine: (id: string) => void;
	compileTex: (texPath: string, vaultPath: string) => Promise<void>;
	compilingPath: string | null;
	isTexFile: (path: string) => boolean;
};

export function useTexCompile(): TexCompileActions {
	const [engines, setEngines] = useState<LatexEngine[]>([]);
	const [enginesLoading, setEnginesLoading] = useState(false);
	const [selectedEngine, setSelectedEngine] = useState<string | null>(null);
	const [compilingPath, setCompilingPath] = useState<string | null>(null);
	const unlistenRef = useRef<(() => void) | null>(null);

	// Load engines on mount.
	useEffect(() => {
		setEnginesLoading(true);
		commands
			.detectLatexEngines()
			.then((res) => {
				if (res.ok && res.data) {
					setEngines(res.data);
					if (res.data.length > 0) {
						setSelectedEngine(res.data[0].id);
					}
				}
			})
			.catch(() => {
				// Silently ignore — button won't show if no engines found.
			})
			.finally(() => setEnginesLoading(false));

		listen<{ line: string }>("compile:log", () => {
			// Drain log events; log UI can be added later.
		}).then((unlisten) => {
			unlistenRef.current = unlisten;
		});

		return () => {
			unlistenRef.current?.();
		};
	}, []);

	const selectEngine = useCallback((id: string) => {
		setSelectedEngine(id);
	}, []);

	const compileTex = useCallback(
		async (texPath: string, _vaultPath: string) => {
			const engine = selectedEngine;
			if (!engine) {
				notifyError("请先选择一个 LaTeX 引擎");
				return;
			}

			setCompilingPath(texPath);

			try {
				const res = await commands.compileTex(texPath, engine);

				if (!res.ok) {
					notifyError(res.error?.message ?? "编译失败");
					return;
				}

				const result = res.data;
				if (result && result.ok && result.pdf_path) {
					notifySuccess("编译成功");
					openTab(result.pdf_path, {
						preferMode: "pdf" as CenterViewMode,
					});
				} else {
					notifyError("编译失败，请检查日志");
				}
			} catch (e) {
				notifyError(String(e));
			} finally {
				setCompilingPath(null);
			}
		},
		[selectedEngine],
	);

	/**
	 * True when `path` is a .tex file NOT under a papers/ folder.
	 */
	const isTexFile = useCallback((path: string): boolean => {
		if (!/\.tex$/i.test(path)) return false;
		const rel = path.replace(/^.*[/\\]papers[/\\]/, "papers/");
		return !rel.startsWith("papers/");
	}, []);

	return {
		engines,
		enginesLoading,
		selectedEngine,
		selectEngine,
		compileTex,
		compilingPath,
		isTexFile,
	};
}
