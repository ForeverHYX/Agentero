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

import { notifyError } from "@/lib/core/notify";
import { vaultStore } from "@/lib/vault/store";
// Import after the mocks: the actions under test save through mocked barrels.
import { compileTexOnSave, persistTextFile } from "@/lib/workspace/actions";
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
	vi.mocked(notifyError).mockClear();
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

describe("persistTextFile (autosave) never compiles", () => {
	it("writes a .tex save without triggering a compile", async () => {
		await persistTextFile(TEX_PATH, "\\documentclass", SEED);

		expect(writeVaultFileMock).toHaveBeenCalledWith(
			TEX_PATH,
			"\\documentclass",
		);
		await flushMicrotasks();
		expect(compileTexFileMock).not.toHaveBeenCalled();
	});
});

describe("compileTexOnSave (manual ⌘S trigger)", () => {
	it("compiles quietly and refreshes the open PDF pane in place", async () => {
		seedPdfPane();
		const bytes = new ArrayBuffer(64);
		compileTexFileMock.mockResolvedValue(PDF_PATH);
		localFileToArrayBufferMock.mockResolvedValue(bytes);

		await compileTexOnSave(TEX_PATH);
		await vi.waitFor(() => {
			expect(pdfTab()?.pdfBytes).toBe(bytes);
		});

		expect(compileTexFileMock).toHaveBeenCalledWith(TEX_PATH, {
			quietSuccess: true,
			triggerPath: TEX_PATH,
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

		await compileTexOnSave(TEX_PATH);
		await vi.waitFor(() => {
			expect(pdfTab()?.texCompiling).toBeFalsy();
		});

		expect(pdfTab()?.pdfBytes).toBe(oldBytes);
		expect(localFileToArrayBufferMock).not.toHaveBeenCalled();
	});

	it("queues one trailing compile for triggers landing mid-compile", async () => {
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

		// ⌘S fires the compile detached (the workspace layer calls it void).
		void compileTexOnSave(TEX_PATH);
		await vi.waitFor(() => {
			expect(compileTexFileMock).toHaveBeenCalledTimes(1);
		});
		// Shimmer gates partial watcher writes while latexmk runs.
		expect(pdfTab()?.texCompiling).toBe(true);

		// A second ⌘S lands while the first compile is in flight.
		void compileTexOnSave(TEX_PATH);
		await flushMicrotasks();
		expect(compileTexFileMock).toHaveBeenCalledTimes(1);

		resolveFirst(PDF_PATH);
		await vi.waitFor(() => {
			expect(compileTexFileMock).toHaveBeenCalledTimes(2);
		});
		expect(compileTexFileMock).toHaveBeenLastCalledWith(TEX_PATH, {
			quietSuccess: true,
			triggerPath: TEX_PATH,
		});
	});

	it("stays silent when no LaTeX engine is available", async () => {
		texCompileStore.setState({
			engines: [],
			selectedEngine: null,
			compilingPath: null,
		});

		await compileTexOnSave(TEX_PATH);
		await flushMicrotasks();

		expect(compileTexFileMock).not.toHaveBeenCalled();
	});

	it("waits for the in-flight engine scan instead of dropping the trigger", async () => {
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

		await compileTexOnSave(TEX_PATH);
		await vi.waitFor(() => {
			expect(compileTexFileMock).toHaveBeenCalledTimes(1);
		});
		await vi.waitFor(() => {
			expect(pdfTab()?.pdfBytes).toBe(bytes);
		});
	});
});

describe("manual save surfaces a missing engine", () => {
	it("notifies instead of staying silent when ⌘S has no engine", async () => {
		texCompileStore.setState({
			engines: [],
			selectedEngine: null,
			compilingPath: null,
		});

		const { compileTexOnManualSave } = await import("@/lib/workspace/actions");
		await compileTexOnManualSave(TEX_PATH);
		await flushMicrotasks();

		expect(compileTexFileMock).not.toHaveBeenCalled();
		expect(notifyError).toHaveBeenCalledTimes(1);
	});

	it("compiles .tex after ⌘S and ignores other extensions", async () => {
		const { compileTexOnManualSave } = await import("@/lib/workspace/actions");
		await compileTexOnManualSave(TEX_PATH);
		await vi.waitFor(() => {
			expect(compileTexFileMock).toHaveBeenCalledTimes(1);
		});

		await compileTexOnManualSave(`${VAULT}/plans/a.txt`);
		await flushMicrotasks();
		expect(compileTexFileMock).toHaveBeenCalledTimes(1);
		expect(notifyError).not.toHaveBeenCalled();
	});
});

async function flushMicrotasks(): Promise<void> {
	await new Promise<void>((resolve) => setTimeout(resolve, 0));
}
