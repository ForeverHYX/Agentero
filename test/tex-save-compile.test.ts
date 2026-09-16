import { beforeEach, describe, expect, it, vi } from "vitest";

const readVaultFileMock = vi.hoisted(() => vi.fn());
const writeVaultFileMock = vi.hoisted(() => vi.fn());
const localFileToArrayBufferMock = vi.hoisted(() => vi.fn());
const compileTexFileMock = vi.hoisted(() => vi.fn());
const ensureTexEnginesMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/core/tauri", async (importOriginal) => {
	const actual = await importOriginal<typeof import("@/lib/core/tauri")>();
	return { ...actual, isTauri: () => true };
});

vi.mock("@/lib/vault", async (importOriginal) => {
	const actual = await importOriginal<typeof import("@/lib/vault")>();
	return {
		...actual,
		readVaultFile: readVaultFileMock,
		writeVaultFile: writeVaultFileMock,
	};
});

vi.mock("@/lib/paper", async (importOriginal) => {
	const actual = await importOriginal<typeof import("@/lib/paper")>();
	return { ...actual, localFileToArrayBuffer: localFileToArrayBufferMock };
});

vi.mock("@/lib/core/notify", () => ({
	notifyError: vi.fn(),
	notifyUndo: vi.fn(),
	notifyWarning: vi.fn(),
	notifySuccess: vi.fn(),
}));

vi.mock("@/lib/workspace/tex-compile", async (importOriginal) => {
	const actual =
		await importOriginal<typeof import("@/lib/workspace/tex-compile")>();
	return {
		...actual,
		compileTexFile: compileTexFileMock,
		ensureTexEngines: ensureTexEnginesMock,
	};
});

import { vaultStore } from "@/lib/vault/store";
// Import after the mocks: persistTextFile saves through the mocked barrels.
import { persistTextFile } from "@/lib/workspace/actions";
import { workspaceStore } from "@/lib/workspace/store";
import { createPlaceholderTab, tabIdForPath } from "@/lib/workspace/tabs";
import { texCompileStore } from "@/lib/workspace/tex-compile";
import { texPdfPath } from "@/lib/workspace/viewer";

const VAULT = "/Users/philfan/l/paper";
const TEX_PATH = `${VAULT}/plans/a.tex`;
const PDF_PATH = texPdfPath(TEX_PATH);
const PDF_ID = tabIdForPath(PDF_PATH);
// Disk snapshot returned by readVaultFile; passed as `lastSaved` so the
// conflict guard sees an unchanged file and lets the save through.
const SEED = "seed-on-disk";
const ENGINE = { id: "pdflatex", label: "pdfLaTeX", path: "/usr/bin/pdflatex" };

function seedPdfPane(): ArrayBuffer {
	const oldBytes = new ArrayBuffer(8);
	workspaceStore.setState({
		tabs: [
			{
				...createPlaceholderTab(PDF_PATH, "pdf"),
				loaded: true,
				pdfBytes: oldBytes,
			},
		],
		activeTabId: null,
	});
	return oldBytes;
}

function pdfTab() {
	return workspaceStore.getState().tabs.find((t) => t.id === PDF_ID);
}

beforeEach(() => {
	readVaultFileMock.mockReset().mockResolvedValue(SEED);
	writeVaultFileMock.mockReset().mockResolvedValue(undefined);
	localFileToArrayBufferMock.mockReset().mockResolvedValue(null);
	compileTexFileMock.mockReset().mockResolvedValue(null);
	ensureTexEnginesMock.mockReset().mockResolvedValue(undefined);
	texCompileStore.setState({
		engines: [ENGINE],
		enginesLoading: false,
		selectedEngine: ENGINE.id,
		compilingPath: null,
	});
	workspaceStore.setState({ tabs: [], activeTabId: null });
	vaultStore.setState({ vaultPath: VAULT });
});

describe("persistTextFile → save-triggered compile", () => {
	it("compiles after a .tex save lands on disk (quiet success)", async () => {
		await persistTextFile(TEX_PATH, "\\documentclass", SEED);

		expect(await persistTexCompileScheduled()).toBe(true);
		expect(compileTexFileMock).toHaveBeenCalledWith(TEX_PATH, {
			quietSuccess: true,
		});
	});

	it("ignores saves of non-TeX files", async () => {
		await persistTextFile(`${VAULT}/plans/a.txt`, "hello", SEED);
		await flushMicrotasks();

		expect(compileTexFileMock).not.toHaveBeenCalled();
	});

	it("refreshes the open PDF pane in place when the compile lands", async () => {
		seedPdfPane();
		const bytes = new ArrayBuffer(64);
		compileTexFileMock.mockResolvedValue(PDF_PATH);
		localFileToArrayBufferMock.mockResolvedValue(bytes);

		await persistTextFile(TEX_PATH, "\\documentclass", SEED);
		await vi.waitFor(() => {
			expect(pdfTab()?.pdfBytes).toBe(bytes);
		});

		const tab = pdfTab();
		expect(tab?.loaded).toBe(true);
		expect(tab?.texCompiling).toBeFalsy();
		expect(tab?.title).toBe("a.pdf");
		expect(localFileToArrayBufferMock).toHaveBeenCalledWith(PDF_PATH);
	});

	it("clears the shimmer and keeps previous bytes when the compile fails", async () => {
		const oldBytes = seedPdfPane();
		compileTexFileMock.mockResolvedValue(null);

		await persistTextFile(TEX_PATH, "\\documentclass", SEED);
		await vi.waitFor(() => {
			expect(pdfTab()?.texCompiling).toBeFalsy();
		});

		expect(pdfTab()?.pdfBytes).toBe(oldBytes);
		expect(localFileToArrayBufferMock).not.toHaveBeenCalled();
	});

	it("queues one trailing compile for saves landing mid-compile", async () => {
		seedPdfPane();
		let resolveFirst!: (value: string | null) => void;
		compileTexFileMock.mockImplementationOnce(() => {
			texCompileStore.setState({ compilingPath: TEX_PATH });
			return new Promise<string | null>((resolve) => {
				resolveFirst = resolve;
			}).finally(() => {
				texCompileStore.setState({ compilingPath: null });
			});
		});
		// Trailing run: fail quietly (asserted via the second call).
		compileTexFileMock.mockResolvedValue(null);

		await persistTextFile(TEX_PATH, "v1", SEED);
		expect(compileTexFileMock).toHaveBeenCalledTimes(1);
		// Shimmer gates partial watcher writes while latexmk runs.
		expect(pdfTab()?.texCompiling).toBe(true);

		// Second autosave lands while the first compile is in flight.
		await persistTextFile(TEX_PATH, "v2", SEED);
		expect(compileTexFileMock).toHaveBeenCalledTimes(1);

		resolveFirst(PDF_PATH);
		await vi.waitFor(() => {
			expect(compileTexFileMock).toHaveBeenCalledTimes(2);
		});
		expect(compileTexFileMock).toHaveBeenLastCalledWith(TEX_PATH, {
			quietSuccess: true,
		});
	});

	it("stays silent when no LaTeX engine is available", async () => {
		texCompileStore.setState({
			engines: [],
			selectedEngine: null,
			compilingPath: null,
		});

		await persistTextFile(TEX_PATH, "\\documentclass", SEED);
		await flushMicrotasks();

		expect(compileTexFileMock).not.toHaveBeenCalled();
	});

	it("waits for the in-flight engine scan instead of dropping the save", async () => {
		seedPdfPane();
		const bytes = new ArrayBuffer(32);
		compileTexFileMock.mockResolvedValue(PDF_PATH);
		localFileToArrayBufferMock.mockResolvedValue(bytes);
		// Reload race: detection still pending, engine list empty at save time.
		texCompileStore.setState({
			engines: [],
			selectedEngine: null,
			compilingPath: null,
		});
		ensureTexEnginesMock.mockImplementationOnce(async () => {
			texCompileStore.setState({
				engines: [ENGINE],
				selectedEngine: ENGINE.id,
			});
		});

		await persistTextFile(TEX_PATH, "\\documentclass", SEED);
		await vi.waitFor(() => {
			expect(compileTexFileMock).toHaveBeenCalledTimes(1);
		});
		await vi.waitFor(() => {
			expect(pdfTab()?.pdfBytes).toBe(bytes);
		});
	});
});

/** persistTextFile fires the compile as a detached async — flush the trigger. */
async function persistTexCompileScheduled(): Promise<boolean> {
	await vi.waitFor(() => {
		expect(compileTexFileMock).toHaveBeenCalledTimes(1);
	});
	return true;
}

async function flushMicrotasks(): Promise<void> {
	await new Promise<void>((resolve) => setTimeout(resolve, 0));
}
