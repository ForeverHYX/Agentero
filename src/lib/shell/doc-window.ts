/**
 * Per-path document native windows.
 */

import i18n from "@/i18n";
import { commands } from "@/lib/core/bindings";
import { notifyError } from "@/lib/core/notify";
import { isTauri } from "@/lib/core/tauri";
import { getVaultPath } from "@/lib/vault/store";

/** Open or focus a document window for `path`. */
export async function openDocWindow(
	path: string,
	mode?: string | null,
	opts?: { title?: string | null },
): Promise<void> {
	if (!isTauri()) {
		notifyError(i18n.t("app:windows.docDesktopOnly"));
		return;
	}
	try {
		const fileName = path.split(/[/\\]/).pop() || undefined;
		const res = await commands.docWindowOpen(
			path,
			mode ?? null,
			getVaultPath(),
			opts?.title?.trim() || fileName || i18n.t("app:windows.titleDocument"),
		);
		if (res.status === "error") {
			notifyError(res.error);
		}
	} catch (e) {
		notifyError(String(e));
	}
}

export function readDocWindowParams(): {
	path: string | null;
	mode: string | null;
	vaultPath: string | null;
} {
	try {
		const params = new URLSearchParams(window.location.search);
		return {
			path: params.get("path"),
			mode: params.get("mode"),
			vaultPath: params.get("vault_path"),
		};
	} catch {
		return { path: null, mode: null, vaultPath: null };
	}
}
