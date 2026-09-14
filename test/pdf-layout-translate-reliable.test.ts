import { describe, expect, it } from "vitest";
import {
	LAYOUT_TRANSLATE_MAX_CHARS,
	listTranslatableLayoutRegions,
	splitLongLayoutTranslateSource,
} from "@/lib/pdf/layout/layout-translate-reliable";
import type { PdfLayoutRegion } from "@/lib/pdf/layout/types";

describe("PDF layout translation reliability", () => {
	it("excludes formulas and image OCR from the runtime translation source", () => {
		const regions: PdfLayoutRegion[] = [
			{
				id: "formula",
				kind: "formula",
				label: "formula",
				score: 0.9,
				pageIndex: 0,
				readingOrder: 0,
				rect: { x: 10, y: 10, w: 80, h: 15 },
				bbox: { x: 0.1, y: 0.1, w: 0.8, h: 0.05 },
			},
			{
				id: "formula-text",
				kind: "text",
				label: "text",
				score: 0.9,
				pageIndex: 0,
				readingOrder: 1,
				rect: { x: 8, y: 8, w: 90, h: 25 },
				bbox: { x: 0.08, y: 0.08, w: 0.84, h: 0.08 },
				text: "The equation x = f(y) stays visible.",
			},
			{
				id: "image",
				kind: "image",
				label: "image",
				score: 0.9,
				pageIndex: 0,
				readingOrder: 2,
				rect: { x: 10, y: 40, w: 80, h: 40 },
				bbox: { x: 0.1, y: 0.4, w: 0.8, h: 0.3 },
			},
			{
				id: "image-ocr",
				kind: "text",
				label: "text",
				score: 0.9,
				pageIndex: 0,
				readingOrder: 3,
				rect: { x: 20, y: 50, w: 30, h: 10 },
				bbox: { x: 0.2, y: 0.5, w: 0.3, h: 0.05 },
				text: "Text painted in the figure",
			},
			{
				id: "caption",
				kind: "figure_title",
				label: "figure_title",
				score: 0.9,
				pageIndex: 0,
				readingOrder: 4,
				rect: { x: 10, y: 82, w: 80, h: 8 },
				bbox: { x: 0.1, y: 0.72, w: 0.8, h: 0.04 },
				text: "Figure 2: Caption remains translatable.",
			},
		];

		expect(
			listTranslatableLayoutRegions(regions).map((item) => item.id),
		).toEqual(["caption"]);
	});

	it("keeps oversized source instead of truncating it", () => {
		const sentences = Array.from(
			{ length: 80 },
			(_, index) =>
				`Sentence ${index} carries source text that must survive splitting.`,
		);
		const source = sentences.join(" ");
		const chunks = splitLongLayoutTranslateSource(source);

		expect(chunks.length).toBeGreaterThan(1);
		expect(
			chunks.every((chunk) => chunk.length <= LAYOUT_TRANSLATE_MAX_CHARS),
		).toBe(true);
		expect(chunks.join(" ")).toBe(source);
	});

	it("hard-splits unbroken text without losing characters", () => {
		const source = "x".repeat(LAYOUT_TRANSLATE_MAX_CHARS * 2 + 137);
		const chunks = splitLongLayoutTranslateSource(source);

		expect(chunks.map((chunk) => chunk.length)).toEqual([
			LAYOUT_TRANSLATE_MAX_CHARS,
			LAYOUT_TRANSLATE_MAX_CHARS,
			137,
		]);
		expect(chunks.join("")).toBe(source);
	});
});
