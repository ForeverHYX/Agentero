import { afterEach, describe, expect, it, vi } from "vitest";

import { webViewProxyOrigins, webViewProxyUrl } from "@/lib/web-view/proxy-url";

function stubUserAgent(value: string) {
	vi.stubGlobal("navigator", { userAgent: value });
}

describe("webViewProxyUrl", () => {
	afterEach(() => vi.unstubAllGlobals());

	it("puts the host in the first path segment (mac/Linux scheme)", () => {
		stubUserAgent("Mozilla/5.0 (Macintosh)");
		expect(webViewProxyUrl("https://blog.nature.com/posts/x?pg=2")).toBe(
			"agentero-web://localhost/blog.nature.com/posts/x?pg=2",
		);
	});

	it("uses the WebView2 http form on Windows", () => {
		stubUserAgent("Mozilla/5.0 (Windows NT 10.0; Win64; x64)");
		expect(webViewProxyUrl("https://arxiv.org/abs/2401.00001")).toBe(
			"http://agentero-web.localhost/arxiv.org/abs/2401.00001",
		);
	});

	it("keeps the query string and drops the hash", () => {
		stubUserAgent("Mozilla/5.0 (Macintosh)");
		expect(webViewProxyUrl("https://x.com/a?b=1#frag")).toBe(
			"agentero-web://localhost/x.com/a?b=1",
		);
	});

	it("rejects non-http(s) URLs", () => {
		expect(() => webViewProxyUrl("file:///etc/passwd")).toThrow();
	});

	it("survives a missing navigator (SSR guard)", () => {
		vi.stubGlobal("navigator", undefined);
		expect(webViewProxyUrl("https://x.com/a")).toBe(
			"agentero-web://localhost/x.com/a",
		);
	});
});

describe("webViewProxyOrigins", () => {
	it("accepts both platform origin forms everywhere", () => {
		expect(webViewProxyOrigins()).toEqual([
			"agentero-web://localhost",
			"http://agentero-web.localhost",
		]);
	});
});
