/**
 * `agentero-web` proxy URL shaping for the HTML paper viewer.
 *
 * The Host registers the scheme (`features/web/proxy.rs`) and puts the upstream
 * host in the first path segment, so relative links inside the page stay on the
 * proxy. Windows WebView2 intercepts custom schemes as `http://<scheme>.localhost`
 * (same caveat as `arxivReaderUrl`).
 */

/** Map a remote `http(s)` URL onto the proxy frame src. */
export function webViewProxyUrl(url: string): string {
	const parsed = new URL(url);
	if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
		throw new Error(`webViewProxyUrl: unsupported protocol ${parsed.protocol}`);
	}
	// Read the platform per call (arxivReaderUrl style) so tests can stub it.
	const isWindows =
		typeof navigator !== "undefined" && navigator.userAgent.includes("Windows");
	const origin = isWindows
		? "http://agentero-web.localhost"
		: "agentero-web://localhost";
	return `${origin}/${parsed.hostname}${parsed.pathname}${parsed.search}`;
}

/**
 * Origins a bridge `MessageEvent` may carry — checked against `event.origin`
 * so an unrelated site's frame cannot spoof `source: "agentero-web"`.
 *
 * Both platform forms are accepted everywhere: WebView2 serializes the frame
 * origin as `http://agentero-web.localhost`, WebKit as `agentero-web://localhost`,
 * and no page outside our proxy can hold either.
 */
export function webViewProxyOrigins(): string[] {
	return ["agentero-web://localhost", "http://agentero-web.localhost"];
}
