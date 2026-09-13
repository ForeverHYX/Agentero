/**
 * Borderless (exclusive) fullscreen for the current window.
 * Windows-only: other platforms keep OS-native maximize / green-button fullscreen.
 */

import { errorText } from "@/lib/core/error";
import { logger } from "@/lib/core/logger";
import { isTauri, isWindows } from "@/lib/core/tauri";

/** Toggle Tauri `setFullscreen` on the focused window (no-op off Windows). */
export async function toggleBorderlessFullscreen(): Promise<void> {
	if (!isTauri() || !isWindows()) return;
	try {
		const { getCurrentWindow } = await import("@tauri-apps/api/window");
		const win = getCurrentWindow();
		const full = await win.isFullscreen();
		const next = !full;
		await win.setFullscreen(next);
		if ((await win.isFullscreen()) !== next) {
			throw new Error(`fullscreen state did not change to ${next}`);
		}
	} catch (error) {
		logger.warn("toggle borderless fullscreen failed", {
			error: errorText(error),
		});
	}
}
