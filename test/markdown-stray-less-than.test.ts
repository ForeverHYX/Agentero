import { MarkdownPlugin } from "@platejs/markdown";
import { createSlateEditor } from "platejs";
import { describe, expect, it } from "vitest";

import { MarkdownKit } from "@/components/editor/plugins/markdown-kit";
import {
	escapeStrayLessThan,
	prepareMarkdownForDeserialize,
} from "@/lib/markdown/deserialize";

describe("escapeStrayLessThan", () => {
	it("escapes a `<` that would crash the MDX JSX tokenizer", () => {
		expect(escapeStrayLessThan("网络（<0.5B，NPU）上")).toBe(
			"网络（&lt;0.5B，NPU）上",
		);
		expect(escapeStrayLessThan("p<0.05 <3 a<-1 x<=2")).toBe(
			"p&lt;0.05 &lt;3 a&lt;-1 x&lt;=2",
		);
		expect(escapeStrayLessThan("尾随 <")).toBe("尾随 &lt;");
		expect(escapeStrayLessThan("broken closing </3 tag")).toBe(
			"broken closing &lt;/3 tag",
		);
	});

	it("keeps tags, fragments, comments and spaced comparisons intact", () => {
		const tags = '<div class="x">a</div> <> <!-- note --> </span> a < b';
		expect(escapeStrayLessThan(tags)).toBe(tags);
	});

	it("leaves code fences, block math and inline code/math untouched", () => {
		const source = [
			"```",
			"a<0.5",
			"```",
			"$$",
			"x<0.5",
			"$$",
			"code `<0.5` and math $a<0.5$ stay",
		].join("\n");
		expect(escapeStrayLessThan(source)).toBe(source);
	});

	it("ignores an indented code-looking line only via fences, math still tracked", () => {
		// Fenced code lines are skipped even when they contain `$$`.
		const source = "```\n$$\n```\noutside <0.5";
		expect(escapeStrayLessThan(source)).toBe("```\n$$\n```\noutside &lt;0.5");
	});
});

describe("#533 note truncated at prose `<` comparison", () => {
	const ISSUE_BODY = `## Problem Definition

**目标问题**：在单个紧凑网络（<0.5B，可跑在手机 NPU 上）内统一支持 T2I 生成与指令式图像编辑，并保持可接受的延迟（$1024\\times1024$ < 1s）。

**为什么重要**：
- 服务端统一模型（FLUX、BAGEL 等）动辄 4B–12B，端侧不可部署；

**已有路线及其瓶颈**：
1. **InstructPix2Pix 式通道拼接**：破坏预训练 T2I 模型的生成先验。
2. **大模型上下文范式**：统一但需要大 backbone，端侧放不下。
`;

	function deserialize(source: string) {
		return createSlateEditor({ plugins: MarkdownKit })
			.getApi(MarkdownPlugin)
			.markdown.deserialize(prepareMarkdownForDeserialize(source));
	}

	function allText(nodes: unknown[]): string {
		return (
			JSON.stringify(nodes)
				.match(/"text":"[^"]*"/g)
				?.join(" ") ?? ""
		);
	}

	it("keeps every block after the paragraph containing `<0.5B`", () => {
		const flat = allText(deserialize(ISSUE_BODY));
		expect(flat).toContain("<0.5B");
		expect(flat).toContain("Problem Definition");
		expect(flat).toContain("为什么重要");
		expect(flat).toContain("InstructPix2Pix");
		expect(flat).toContain("大模型上下文范式");
	});

	it("round-trips: the escaped `<` survives a serialize → deserialize cycle", () => {
		const value = deserialize("A（<0.5B）x\n\nafter");
		const serialized = createSlateEditor({ plugins: MarkdownKit, value })
			.getApi(MarkdownPlugin)
			.markdown.serialize();
		expect(allText(deserialize(serialized))).toContain("<0.5B");
	});
});
