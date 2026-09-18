//! Process-wide HTTP plumbing shared by Host features and the headless CLI:
//! proxy configuration, client factories, the product User-Agent, and
//! error-body truncation.

use crate::error::AppError;
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

/// Product User-Agent sent by Host HTTP clients by default.
///
/// The repo + mailto contacts keep Crossref / Semantic Scholar requests in
/// their polite pools.
pub const USER_AGENT: &str = concat!(
    "Agentero/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/poco-ai/agentero; mailto:agentero@users.noreply.github.com)"
);

/// Browser-like UA for endpoints that reject non-browser agents with HTTP 403
/// (PLOS / IEEE / Springer publisher PDFs, free web-MT endpoints). Use only
/// where a browser is deliberately impersonated; prefer [`USER_AGENT`]
/// everywhere else.
pub const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";

/// Redirect cap applied by [`client`] (reqwest's own default is 10).
pub const DEFAULT_REDIRECT_LIMIT: usize = 5;

/// How many chars of an HTTP error body [`http_err_snippet`] keeps.
const ERROR_SNIPPET_CHARS: usize = 180;

static PROXY_URL: OnceLock<RwLock<Option<String>>> = OnceLock::new();
static SHARED_CLIENT: OnceLock<RwLock<Option<CachedClient>>> = OnceLock::new();
/// Optional URL-prefix mirror for GitHub download hosts (Skill import, etc.).
/// `None` = disabled. Value is a trimmed base without a trailing slash, e.g.
/// `https://gh.llkk.cc` → requests become `{base}/https://codeload.github.com/...`.
static GITHUB_MIRROR: OnceLock<RwLock<Option<String>>> = OnceLock::new();
/// Cached GitHub token discovered from environment variables or `gh auth token`.
/// The token is never logged and is only attached to direct `api.github.com`
/// requests, never to third-party mirror URLs.
static GITHUB_AUTH_TOKEN: OnceLock<RwLock<CachedGithubToken>> = OnceLock::new();
/// (last detected OS system proxy, when it was checked). `None` timestamp =
/// never checked.
static SYSTEM_PROXY: OnceLock<RwLock<(Option<String>, Option<Instant>)>> = OnceLock::new();

/// The OS-level proxy can be toggled at runtime (Clash / V2RayN "system
/// proxy" mode); re-read it at most this often.
const SYSTEM_PROXY_TTL: Duration = Duration::from_secs(30);
const GITHUB_AUTH_TOKEN_TTL: Duration = Duration::from_secs(300);

struct CachedClient {
    proxy: Option<String>,
    client: reqwest::Client,
}

#[derive(Default)]
struct CachedGithubToken {
    token: Option<String>,
    checked_at: Option<Instant>,
}

fn proxy_slot() -> &'static RwLock<Option<String>> {
    PROXY_URL.get_or_init(|| RwLock::new(None))
}

/// Configure the proxy used by every Host-created reqwest client.
pub fn configure_proxy(enabled: bool, url: &str) -> Result<(), AppError> {
    let normalized = url.trim().to_string();
    let next = if enabled {
        if normalized.is_empty() {
            return Err(AppError::message("network proxy URL is required"));
        }
        reqwest::Proxy::all(&normalized)
            .map_err(|e| AppError::message(format!("invalid network proxy URL: {e}")))?;
        Some(normalized)
    } else {
        None
    };

    let mut guard = proxy_slot()
        .write()
        .map_err(|_| AppError::message("network proxy lock poisoned"))?;
    *guard = next;
    drop(guard);
    if let Some(slot) = SHARED_CLIENT.get() {
        if let Ok(mut cached) = slot.write() {
            *cached = None;
        }
    }
    Ok(())
}

fn cached_slot() -> &'static RwLock<Option<CachedClient>> {
    SHARED_CLIENT.get_or_init(|| RwLock::new(None))
}

/// The OS-wide ("system") proxy, if one is configured. reqwest only honors
/// `HTTP_PROXY`-style env vars, so without this every Host request would
/// bypass the system proxy that browsers and WebView2 use — the classic
/// "browser works, app pages fail" split on Windows proxy clients.
pub fn system_proxy_url() -> Option<String> {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu
            .open_subkey_with_flags(
                r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
                KEY_READ,
            )
            .ok()?;
        let enabled: u32 = key.get_value("ProxyEnable").ok()?;
        if enabled == 0 {
            return None;
        }
        let server: String = key.get_value("ProxyServer").ok()?;
        parse_windows_proxy_server(&server)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Parse the Windows `ProxyServer` registry value: either a bare `host:port`
/// or `http=host:port;https=host:port;socks=host:port`.
// Only called from the Windows branch + unit tests; keep it compiled (and
// tested) on every platform.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn parse_windows_proxy_server(server: &str) -> Option<String> {
    let trimmed = server.trim();
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed.contains('=') {
        return Some(with_proxy_scheme(trimmed, "http"));
    }
    let mut http = None;
    let mut https = None;
    let mut socks = None;
    for part in trimmed.split(';') {
        let Some((kind, value)) = part.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match kind.trim().to_ascii_lowercase().as_str() {
            "http" => http = Some(value),
            "https" => https = Some(value),
            "socks" => socks = Some(value),
            _ => {}
        }
    }
    // The `http=`/`https=` keys name which traffic a proxy serves, not how the
    // proxy itself is reached — the proxy is contacted over plain HTTP (HTTPS
    // targets tunnel via CONNECT). So both map to an `http://` proxy URL;
    // returning `https://` would make reqwest TLS-handshake with a plain HTTP
    // proxy and fail. Prefer the `http=` entry, then `https=`.
    http.or(https)
        .map(|v| with_proxy_scheme(v, "http"))
        .or_else(|| socks.map(|v| with_proxy_scheme(v, "socks5h")))
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn with_proxy_scheme(value: &str, default_scheme: &str) -> String {
    if value.contains("://") {
        value.to_string()
    } else {
        format!("{default_scheme}://{value}")
    }
}

/// TTL-cached [`system_proxy_url`] so request paths only touch the registry
/// every [`SYSTEM_PROXY_TTL`].
fn system_proxy_cached() -> Option<String> {
    let slot = SYSTEM_PROXY.get_or_init(|| RwLock::new((None, None)));
    if let Ok(guard) = slot.read() {
        if guard.1.is_some_and(|t| t.elapsed() < SYSTEM_PROXY_TTL) {
            return guard.0.clone();
        }
    }
    let detected = system_proxy_url();
    if let Ok(mut guard) = slot.write() {
        *guard = (detected.clone(), Some(Instant::now()));
    }
    detected
}

/// The proxy every Host-created client should use: the explicit setting when
/// enabled, otherwise the detected system proxy. Also the value forwarded to
/// Host-spawned subprocesses so in-app and CLI network behavior match.
pub fn effective_proxy_url() -> Option<String> {
    if let Ok(guard) = proxy_slot().read() {
        if guard.is_some() {
            return guard.clone();
        }
    }
    system_proxy_cached()
}

/// A process-wide reqwest client so TLS sessions and HTTP keep-alive survive
/// across plaza / Cool Papers requests. Rebuilt when the effective proxy
/// changes (explicit setting or detected system proxy).
pub fn shared_client() -> Result<reqwest::Client, AppError> {
    let proxy = effective_proxy_url();
    {
        let guard = cached_slot()
            .read()
            .map_err(|_| AppError::message("network client lock poisoned"))?;
        if let Some(cached) = guard.as_ref() {
            if cached.proxy == proxy {
                return Ok(cached.client.clone());
            }
        }
    }
    let client = client_builder()
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(8)
        .redirect(reqwest::redirect::Policy::limited(DEFAULT_REDIRECT_LIMIT))
        .build()
        .map_err(|e| AppError::message(format!("http client: {e}")))?;
    let mut guard = cached_slot()
        .write()
        .map_err(|_| AppError::message("network client lock poisoned"))?;
    if let Some(cached) = guard.as_ref() {
        if cached.proxy == proxy {
            return Ok(cached.client.clone());
        }
    }
    *guard = Some(CachedClient {
        proxy,
        client: client.clone(),
    });
    Ok(client)
}

/// Build a reqwest client builder with the current process-wide proxy.
///
/// Prefer [`client`] / [`client_with`]; reach for this directly only when a
/// flow must deviate from their defaults (e.g. no timeout at all).
pub fn client_builder() -> reqwest::ClientBuilder {
    let proxy = effective_proxy_url();
    let builder = reqwest::Client::builder();
    match proxy {
        Some(url) => match reqwest::Proxy::all(&url) {
            Ok(proxy) => builder.proxy(proxy),
            Err(error) => {
                log::error!(target: "agentero::network", "invalid configured proxy: {error}");
                builder
            }
        },
        None => builder,
    }
}

/// Standard Host HTTP client: [`USER_AGENT`], the configured proxy, `timeout`,
/// and at most [`DEFAULT_REDIRECT_LIMIT`] redirects.
pub fn client(timeout: Duration) -> Result<reqwest::Client, AppError> {
    client_with(timeout, DEFAULT_REDIRECT_LIMIT, USER_AGENT)
}

/// [`client`] with an explicit redirect cap and User-Agent — for deeper
/// redirect chains (model / asset downloads) or browser impersonation
/// ([`BROWSER_USER_AGENT`]).
pub fn client_with(
    timeout: Duration,
    redirect_limit: usize,
    user_agent: &str,
) -> Result<reqwest::Client, AppError> {
    client_builder()
        .timeout(timeout)
        .user_agent(user_agent)
        .redirect(reqwest::redirect::Policy::limited(redirect_limit))
        .build()
        .map_err(|e| AppError::message(format!("http client: {e}")))
}

/// First [`ERROR_SNIPPET_CHARS`] chars of an HTTP response body, for embedding
/// in error messages.
pub fn http_err_snippet(text: &str) -> String {
    text.chars().take(ERROR_SNIPPET_CHARS).collect()
}

fn github_mirror_slot() -> &'static RwLock<Option<String>> {
    GITHUB_MIRROR.get_or_init(|| RwLock::new(None))
}

/// Enable / disable the process-wide GitHub URL-prefix mirror.
///
/// When `enabled` is true and `base_url` is non-empty, it must be an absolute
/// `http`/`https` URL. Empty base while enabled soft-disables (no error) so the
/// Settings switch can flip before the user pastes a mirror.
pub fn configure_github_mirror(enabled: bool, base_url: &str) -> Result<(), AppError> {
    let next = if enabled {
        let normalized = base_url.trim().trim_end_matches('/').to_string();
        if normalized.is_empty() {
            None
        } else {
            Some(normalize_github_mirror_base(&normalized)?)
        }
    } else {
        None
    };
    let mut guard = github_mirror_slot()
        .write()
        .map_err(|_| AppError::message("github mirror lock poisoned"))?;
    *guard = next;
    Ok(())
}

fn normalize_github_mirror_base(base: &str) -> Result<String, AppError> {
    let parsed = url::Url::parse(base)
        .map_err(|e| AppError::message(format!("invalid GitHub mirror URL: {e}")))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(AppError::message(format!(
                "GitHub mirror URL must be http(s), got {other}"
            )));
        }
    }
    if parsed.host_str().is_none() {
        return Err(AppError::message("GitHub mirror URL is missing a host"));
    }
    // Drop query/fragment; keep origin (+ optional non-root path without trailing /).
    let mut out = format!(
        "{}://{}",
        parsed.scheme(),
        parsed.host_str().unwrap_or_default()
    );
    if let Some(port) = parsed.port() {
        out.push(':');
        out.push_str(&port.to_string());
    }
    let path = parsed.path().trim_end_matches('/');
    if !path.is_empty() && path != "/" {
        out.push_str(path);
    }
    Ok(out)
}

/// Currently configured mirror base, if any.
pub fn github_mirror_base() -> Option<String> {
    github_mirror_slot().read().ok().and_then(|g| g.clone())
}

fn is_github_download_host(host: &str) -> bool {
    matches!(
        host,
        "github.com"
            | "api.github.com"
            | "codeload.github.com"
            | "raw.githubusercontent.com"
            | "objects.githubusercontent.com"
            | "gist.githubusercontent.com"
    )
}

/// Prefix `canonical` with the mirror base (`{base}/{canonical}`).
pub fn mirror_github_url(base: &str, canonical: &str) -> Option<String> {
    let parsed = url::Url::parse(canonical).ok()?;
    let host = parsed.host_str()?;
    if !is_github_download_host(host) {
        return None;
    }
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let base = base.trim().trim_end_matches('/');
    if base.is_empty() {
        return None;
    }
    Some(format!("{base}/{canonical}"))
}

/// Fetch candidates for a canonical GitHub URL: direct first, then mirror.
pub fn github_url_candidates(canonical: &str) -> Vec<String> {
    let mut out = vec![canonical.to_string()];
    if let Some(base) = github_mirror_base() {
        if let Some(mirrored) = mirror_github_url(&base, canonical) {
            if mirrored != canonical {
                out.push(mirrored);
            }
        }
    }
    out
}

/// Add the local GitHub CLI / env token to direct GitHub REST API requests.
///
/// Mirrors deliberately do not receive the token because URL-prefix mirrors are
/// third-party hosts (`{base}/https://api.github.com/...`).
pub fn with_github_api_auth(
    request: reqwest::RequestBuilder,
    url: &str,
) -> reqwest::RequestBuilder {
    if !is_direct_github_api_url(url) {
        return request;
    }
    match github_auth_token() {
        Some(token) => request.bearer_auth(token),
        None => request,
    }
}

fn is_direct_github_api_url(url: &str) -> bool {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .as_deref()
        == Some("api.github.com")
}

fn github_auth_token() -> Option<String> {
    let slot = GITHUB_AUTH_TOKEN.get_or_init(|| RwLock::new(CachedGithubToken::default()));
    if let Ok(guard) = slot.read() {
        if guard
            .checked_at
            .is_some_and(|t| t.elapsed() < GITHUB_AUTH_TOKEN_TTL)
        {
            return guard.token.clone();
        }
    }

    let token = discover_github_auth_token();
    if let Ok(mut guard) = slot.write() {
        *guard = CachedGithubToken {
            token: token.clone(),
            checked_at: Some(Instant::now()),
        };
    }
    token
}

fn discover_github_auth_token() -> Option<String> {
    for key in ["GH_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(token) = std::env::var(key) {
            let token = token.trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }

    let gh = crate::process::resolve_command("gh")?;
    let output = std::process::Command::new(gh)
        .args(["auth", "token", "--hostname", "github.com"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8(output.stdout).ok()?;
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// Whether an HTTP status should trigger trying the next GitHub mirror candidate.
/// GitHub returns 403 for unauthenticated API quota exhaustion and some edge /
/// regional blocks, so treat it like 429 here. Other client errors (404, etc.)
/// are definitive and must not fall back.
pub fn should_fallback_github_status(status: reqwest::StatusCode) -> bool {
    is_retryable_github_status_code(status.as_u16())
}

fn is_retryable_github_status_code(code: u16) -> bool {
    code == 403 || code == 429 || (500..600).contains(&code)
}

/// Classify `AppError` messages produced by Skill / download helpers for mirror
/// fallback. Prefer status-aware helpers when the status is still in hand.
pub fn should_fallback_github_error(err: &AppError) -> bool {
    let msg = err.to_string();
    if let Some(rest) = msg.strip_prefix("download HTTP ") {
        let code = rest
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .and_then(|s| s.parse::<u16>().ok());
        return matches!(code, Some(c) if is_retryable_github_status_code(c));
    }
    if let Some(rest) = msg.strip_prefix("GitHub repository lookup failed: ") {
        // `StatusCode` Display is like "502 Bad Gateway" or "404 Not Found".
        let code = rest
            .split_whitespace()
            .next()
            .and_then(|s| s.parse::<u16>().ok());
        return matches!(code, Some(c) if is_retryable_github_status_code(c));
    }
    if let Some(rest) = msg.strip_prefix("GitHub contents request failed: ") {
        let code = rest
            .split_whitespace()
            .next()
            .and_then(|s| s.parse::<u16>().ok());
        return matches!(code, Some(c) if is_retryable_github_status_code(c));
    }
    if let Some(rest) = msg.strip_prefix("GitHub blob request failed: ") {
        let code = rest
            .split_whitespace()
            .next()
            .and_then(|s| s.parse::<u16>().ok());
        return matches!(code, Some(c) if is_retryable_github_status_code(c));
    }
    msg.starts_with("download:")
        || msg.starts_with("download body:")
        || msg.starts_with("skill metadata request:")
        || msg.starts_with("skill contents request:")
        || msg.starts_with("skill blob request:")
        || msg.starts_with("http client:")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_supported_proxy_urls() {
        for url in [
            "http://127.0.0.1:7890",
            "https://proxy.example.test:8443",
            "socks5h://127.0.0.1:1080",
        ] {
            configure_proxy(true, url).expect("proxy URL should be accepted");
        }
        configure_proxy(false, "").expect("proxy should be disabled");
    }

    #[test]
    fn rejects_enabled_empty_proxy() {
        let error = configure_proxy(true, " ").expect_err("empty proxy should fail");
        assert!(error.to_string().contains("proxy URL is required"));
    }

    #[test]
    fn parses_windows_proxy_server_forms() {
        assert_eq!(
            parse_windows_proxy_server("127.0.0.1:7890").as_deref(),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(
            parse_windows_proxy_server(
                "http=127.0.0.1:10809;https=127.0.0.1:10809;socks=127.0.0.1:10808"
            )
            .as_deref(),
            Some("http://127.0.0.1:10809")
        );
        assert_eq!(
            parse_windows_proxy_server("socks=127.0.0.1:10808").as_deref(),
            Some("socks5h://127.0.0.1:10808")
        );
        assert_eq!(
            parse_windows_proxy_server("https=proxy.local:8443").as_deref(),
            // https= still yields an http:// proxy URL (see fn comment).
            Some("http://proxy.local:8443")
        );
        assert_eq!(parse_windows_proxy_server("  ").as_deref(), None);
    }

    #[test]
    fn snippet_truncates_long_bodies() {
        let body = "x".repeat(ERROR_SNIPPET_CHARS + 40);
        assert_eq!(http_err_snippet(&body).len(), ERROR_SNIPPET_CHARS);
        assert_eq!(http_err_snippet("short body"), "short body");
    }

    #[test]
    fn normalize_github_mirror_base_strips_trailing_slash() {
        assert_eq!(
            normalize_github_mirror_base("https://gh.llkk.cc/").unwrap(),
            "https://gh.llkk.cc"
        );
        assert_eq!(
            normalize_github_mirror_base("https://example.test:8443/gh").unwrap(),
            "https://example.test:8443/gh"
        );
    }

    #[test]
    fn rejects_non_http_github_mirror() {
        let err = normalize_github_mirror_base("ftp://mirror.test").unwrap_err();
        assert!(err.to_string().contains("http(s)"));
    }

    #[test]
    fn github_url_candidates_direct_then_mirror() {
        configure_github_mirror(false, "").unwrap();
        let only = github_url_candidates("https://codeload.github.com/o/r/tar.gz/main");
        assert_eq!(only.len(), 1);

        configure_github_mirror(true, "https://gh.llkk.cc/").unwrap();
        let both = github_url_candidates("https://api.github.com/repos/o/r");
        assert_eq!(
            both,
            vec![
                "https://api.github.com/repos/o/r".to_string(),
                "https://gh.llkk.cc/https://api.github.com/repos/o/r".to_string(),
            ]
        );
        // Non-GitHub hosts are never mirrored.
        assert_eq!(github_url_candidates("https://example.com/x").len(), 1);
        configure_github_mirror(false, "").unwrap();
    }

    #[test]
    fn github_api_auth_only_targets_direct_api_host() {
        assert!(is_direct_github_api_url("https://api.github.com/repos/o/r"));
        assert!(!is_direct_github_api_url(
            "https://gh.llkk.cc/https://api.github.com/repos/o/r"
        ));
        assert!(!is_direct_github_api_url(
            "https://codeload.github.com/o/r/tar.gz/main"
        ));
    }

    #[test]
    fn fallback_classifies_status_and_messages() {
        assert!(should_fallback_github_status(
            reqwest::StatusCode::TOO_MANY_REQUESTS
        ));
        assert!(should_fallback_github_status(
            reqwest::StatusCode::BAD_GATEWAY
        ));
        assert!(should_fallback_github_status(
            reqwest::StatusCode::FORBIDDEN
        ));
        assert!(!should_fallback_github_status(
            reqwest::StatusCode::NOT_FOUND
        ));

        assert!(should_fallback_github_error(&AppError::message(
            "download: error sending request"
        )));
        assert!(should_fallback_github_error(&AppError::message(
            "download HTTP 502 Bad Gateway"
        )));
        assert!(should_fallback_github_error(&AppError::message(
            "download HTTP 403 Forbidden"
        )));
        assert!(!should_fallback_github_error(&AppError::message(
            "download HTTP 404 Not Found"
        )));
        assert!(should_fallback_github_error(&AppError::message(
            "GitHub repository lookup failed: 403 Forbidden"
        )));
        assert!(!should_fallback_github_error(&AppError::message(
            "GitHub repository lookup failed: 404 Not Found"
        )));
    }
}
