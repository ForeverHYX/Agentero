import { beforeEach, describe, expect, it, vi } from "vitest";

const cleanLatexAuxFilesMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/core/bindings", () => ({
	commands: {
		cleanLatexAuxFiles: cleanLatexAuxFilesMock,
		detectLatexEngines: vi.fn(),
	},
}));

vi.mock("@/lib/core/notify", () => ({
	notifyError: vi.fn(),
	notifySuccess: vi.fn(),
	notifyUndo: vi.fn(),
	notifyWarning: vi.fn(),
}));

import { notifyError, notifySuccess } from "@/lib/core/notify";
// Import after the mocks: cleanTexAuxFiles saves through mocked barrels.
import { cleanTexAuxFiles, texCompileStore } from "@/lib/workspace/tex-compile";

const TEX_PATH = "/Users/philfan/l/paper/thesis/main.tex";

beforeEach(() => {
	vi.mocked(notifyError).mockClear();
	vi.mocked(notifySuccess).mockClear();
	cleanLatexAuxFilesMock.mockReset();
	texCompileStore.setState({
		engines: [],
		enginesLoading: false,
		selectedEngine: null,
		compilingPath: null,
	});
});

describe("cleanTexAuxFiles", () => {
	it("runs latexmk -c for the path and toasts success", async () => {
		cleanLatexAuxFilesMock.mockResolvedValue({ ok: true, data: null });

		const ok = await cleanTexAuxFiles(TEX_PATH);

		expect(ok).toBe(true);
		expect(cleanLatexAuxFilesMock).toHaveBeenCalledWith(TEX_PATH);
		expect(notifySuccess).toHaveBeenCalledTimes(1);
		expect(notifyError).not.toHaveBeenCalled();
	});

	it("surfaces the backend message when the clean fails", async () => {
		cleanLatexAuxFilesMock.mockResolvedValue({
			ok: false,
			data: null,
			error: { code: "message", message: "latexmk not found on this system" },
		});

		const ok = await cleanTexAuxFiles(TEX_PATH);

		expect(ok).toBe(false);
		expect(notifyError).toHaveBeenCalledWith(
			"latexmk not found on this system",
		);
		expect(notifySuccess).not.toHaveBeenCalled();
	});

	it("notifies a fallback when the failure carries no message", async () => {
		cleanLatexAuxFilesMock.mockResolvedValue({
			ok: false,
			data: null,
			error: null,
		});

		const ok = await cleanTexAuxFiles(TEX_PATH);

		expect(ok).toBe(false);
		expect(notifyError).toHaveBeenCalledTimes(1);
	});

	it("notifies the thrown error when the invoke rejects", async () => {
		cleanLatexAuxFilesMock.mockRejectedValue(new Error("ipc gone"));

		const ok = await cleanTexAuxFiles(TEX_PATH);

		expect(ok).toBe(false);
		expect(notifyError).toHaveBeenCalledWith("ipc gone");
	});

	it("is a no-op while a compile is in flight", async () => {
		texCompileStore.setState({ compilingPath: TEX_PATH });

		const ok = await cleanTexAuxFiles(TEX_PATH);

		expect(ok).toBe(false);
		expect(cleanLatexAuxFilesMock).not.toHaveBeenCalled();
		expect(notifySuccess).not.toHaveBeenCalled();
	});
});
