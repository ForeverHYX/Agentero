import { describe, expect, it } from "vitest";
import { embedPdfDocumentId } from "@/lib/pdf";

describe("embedPdfDocumentId", () => {
	it("keeps the base id for url sources without bytes", () => {
		expect(embedPdfDocumentId("tab-1", null)).toBe("tab-1");
		expect(embedPdfDocumentId("tab-1", undefined)).toBe("tab-1");
	});

	it("is stable for the same buffer identity", () => {
		const bytes = new ArrayBuffer(8);
		const first = embedPdfDocumentId("tab-1", bytes);
		expect(embedPdfDocumentId("tab-1", bytes)).toBe(first);
	});

	it("issues a fresh id for a reloaded buffer", () => {
		const before = embedPdfDocumentId("tab-1", new ArrayBuffer(8));
		const after = embedPdfDocumentId("tab-1", new ArrayBuffer(16));
		expect(after).not.toBe(before);
		expect(before.startsWith("tab-1::r")).toBe(true);
		expect(after.startsWith("tab-1::r")).toBe(true);
	});
});
