//! Minimal WebDAV client — exactly what sync needs and nothing more: GET /
//! conditional PUT / DELETE / MKCOL, over Basic auth (works with Nutstore
//! app passwords, Nextcloud app tokens and NAS deployments alike).
//! Implements the [`RemoteStore`] contract; retry/error plumbing is shared
//! in `store.rs`.
//!
//! No XML parsing: the connection test only needs PROPFIND's status code and
//! every object key is our own ASCII-safe layout. Conditional-write support
//! varies: Nutstore ignores `If-None-Match` but enforces `If-Match` (the
//! probe targets `If-Match`, so the HEAD CAS stays atomic there); servers
//! that ignore `If-Match` too degrade to plain PUTs like Aliyun OSS —
//! content-addressed blobs make that idempotent, and convergence comes from
//! the engine's merge retries.

use crate::core::error::AppError;
use crate::core::http;
use crate::integration::sync::config::SyncBackendConfig;
use crate::integration::sync::store::{
    check, etag_of, send_with_retries, PutCondition, PutOutcome, RemoteStore,
};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const PROPFIND_BODY: &[u8] = br#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/></d:prop></d:propfind>"#;

pub struct WebdavClient {
    http: reqwest::Client,
    /// `scheme://host[:port]` of the configured server directory URL.
    origin: String,
    /// Directory path segments below the origin, e.g. `["dav", "agentero"]`.
    dir: Vec<String>,
    username: String,
    password: String,
    /// Seeded from the persisted probe result; most WebDAV servers answer
    /// conditional PUTs with a plain 2xx, which the probe turns into false.
    conditional_writes: AtomicBool,
    /// Directory paths (decoded, `dav/agentero/blobs`) known to exist this
    /// session — WebDAV needs an explicit MKCOL before the first PUT into
    /// a directory, and the layout only ever uses a handful of them.
    known_dirs: Mutex<HashSet<String>>,
}

impl WebdavClient {
    pub fn new(cfg: &SyncBackendConfig) -> Result<Self, AppError> {
        let url = url::Url::parse(cfg.webdav_url.trim())
            .map_err(|e| AppError::message(format!("invalid WebDAV URL: {e}")))?;
        // `scheme://host[:port]` without the path or query.
        let origin = url[..url::Position::BeforePath].to_string();
        let dir = url
            .path()
            .split('/')
            .filter(|s| !s.is_empty())
            .map(percent_decode)
            .collect();
        Ok(Self {
            http: http::client_builder()
                .build()
                .map_err(|e| AppError::message(e.to_string()))?,
            origin,
            dir,
            username: cfg.webdav_username.trim().to_string(),
            password: cfg.webdav_password.clone(),
            conditional_writes: AtomicBool::new(cfg.conditional_writes),
            known_dirs: Mutex::new(HashSet::new()),
        })
    }

    /// Whether conditional writes are (still) assumed to work.
    fn supports_conditional_writes(&self) -> bool {
        self.conditional_writes.load(Ordering::Relaxed)
    }

    /// DELETE an object. 404 counts as success (idempotent cleanup).
    async fn delete(&self, key: &str) -> Result<(), AppError> {
        let resp = self
            .send(method("DELETE")?, &self.url_for(key), &[], Vec::new())
            .await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }
        check(resp, "DELETE", key).await.map(|_| ())
    }

    /// Decoded directory paths from the origin down to the configured dir.
    fn dir_paths(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(self.dir.len());
        let mut acc = String::new();
        for seg in &self.dir {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(seg);
            out.push(acc.clone());
        }
        out
    }

    /// Create every not-yet-known directory of `dirs` (ancestors first).
    async fn mkcol_dirs(&self, dirs: &[String]) -> Result<(), AppError> {
        let mkcol = method("MKCOL")?;
        for dir in dirs {
            if dir.is_empty() || self.dir_known(dir) {
                continue;
            }
            // Trailing slash: the canonical collection form.
            let url = format!("{}/", self.url_for_dir(dir));
            let resp = self.send(mkcol.clone(), &url, &[], Vec::new()).await?;
            let status = resp.status();
            if status.is_success()
                || status == reqwest::StatusCode::METHOD_NOT_ALLOWED
                || status == reqwest::StatusCode::FORBIDDEN
            {
                // 405 = exists. 403 = exists but protected (Nutstore answers
                // MKCOL on its `/dav` root with OperationNotAllowed); a truly
                // read-only location fails loudly on the next PUT instead.
                if let Ok(mut set) = self.known_dirs.lock() {
                    set.insert(dir.clone());
                }
                continue;
            }
            return check(resp, "MKCOL", dir).await.map(|_| ());
        }
        Ok(())
    }

    async fn ensure_parent_dirs(&self, key: &str) -> Result<(), AppError> {
        let segs: Vec<&str> = key.split('/').filter(|s| !s.is_empty()).collect();
        let mut dirs = self.dir_paths();
        let mut acc = dirs.last().cloned().unwrap_or_default();
        for seg in &segs[..segs.len().saturating_sub(1)] {
            if acc.is_empty() {
                acc = (*seg).to_string();
            } else {
                acc = format!("{acc}/{seg}");
            }
            dirs.push(acc.clone());
        }
        // The longest needed directory existing implies its ancestors do too.
        if dirs.last().map(|d| self.dir_known(d)).unwrap_or(true) {
            return Ok(());
        }
        self.mkcol_dirs(&dirs).await
    }

    fn remember_dirs(&self, dirs: &[String]) {
        if let Ok(mut set) = self.known_dirs.lock() {
            set.extend(dirs.iter().cloned());
        }
    }

    fn dir_known(&self, dir: &str) -> bool {
        self.known_dirs
            .lock()
            .map(|set| set.contains(dir))
            .unwrap_or(false)
    }

    /// URL of the configured server directory (collection form).
    fn dir_url(&self) -> String {
        format!("{}/", self.url_for_dir(&self.dir.join("/")))
    }

    /// URL of a decoded directory path relative to the origin.
    fn url_for_dir(&self, dir: &str) -> String {
        let mut url = self.origin.clone();
        for seg in dir.split('/').filter(|s| !s.is_empty()) {
            url.push('/');
            url.push_str(&encode_path_segment(seg));
        }
        url
    }

    /// URL of a store key (`blobs/ab/<sha>` → origin + dir + segments).
    fn url_for(&self, key: &str) -> String {
        let mut url = self.url_for_dir(&self.dir.join("/"));
        for seg in key.split('/').filter(|s| !s.is_empty()) {
            url.push('/');
            url.push_str(&encode_path_segment(seg));
        }
        url
    }

    /// Send one authenticated request; transport-level retries live in
    /// `send_with_retries`.
    async fn send(
        &self,
        method: reqwest::Method,
        url: &str,
        headers: &[(&str, String)],
        body: Vec<u8>,
    ) -> Result<reqwest::Response, AppError> {
        send_with_retries(&method, url, || {
            let mut req = self
                .http
                .request(method.clone(), url)
                .basic_auth(&self.username, Some(&self.password));
            for (name, value) in headers {
                req = req.header(*name, value.clone());
            }
            if !body.is_empty() || method == reqwest::Method::PUT {
                req = req.body(body.clone());
            }
            req
        })
        .await
    }
}

impl RemoteStore for WebdavClient {
    /// Connection test: PROPFIND the configured directory, creating it (and
    /// its parents) when missing so users can point at one that does not
    /// exist yet. Auth failures surface as errors from `check`.
    async fn ensure_root(&self) -> Result<(), AppError> {
        let resp = self
            .send(
                method("PROPFIND")?,
                &self.dir_url(),
                &[("Depth", "0".to_string())],
                PROPFIND_BODY.to_vec(),
            )
            .await?;
        match resp.status() {
            reqwest::StatusCode::MULTI_STATUS => {
                self.remember_dirs(&self.dir_paths());
                Ok(())
            }
            reqwest::StatusCode::NOT_FOUND => self.mkcol_dirs(&self.dir_paths()).await,
            _ => check(resp, "PROPFIND", &self.dir.join("/"))
                .await
                .map(|_| ()),
        }
    }

    /// GET an object. `None` on 404; otherwise `(body, etag)`.
    async fn get(&self, key: &str) -> Result<Option<(Vec<u8>, String)>, AppError> {
        let resp = self
            .send(method("GET")?, &self.url_for(key), &[], Vec::new())
            .await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let resp = check(resp, "GET", key).await?;
        let etag = etag_of(&resp);
        let body = resp
            .bytes()
            .await
            .map_err(|e| AppError::message(format!("GET {key}: {e}")))?;
        Ok(Some((body.to_vec(), etag)))
    }

    async fn put(
        &self,
        key: &str,
        body: Vec<u8>,
        condition: PutCondition,
    ) -> Result<PutOutcome, AppError> {
        self.ensure_parent_dirs(key).await?;
        let cond = if self.supports_conditional_writes() {
            match &condition {
                PutCondition::IfNoneMatch => Some(("If-None-Match", "*".to_string())),
                // A server that returns no ETag cannot back If-Match; fall
                // back to a plain PUT (same degraded-CAS semantics).
                PutCondition::IfMatch(etag) if !etag.is_empty() => Some(("If-Match", etag.clone())),
                PutCondition::IfMatch(_) => None,
            }
        } else {
            None
        };
        let headers: Vec<(&str, String)> = cond.into_iter().collect();
        let resp = self
            .send(method("PUT")?, &self.url_for(key), &headers, body)
            .await?;
        // With parents ensured, a 409 is a quirky server rejecting the
        // conditional header — treat it like the standard 412.
        if resp.status() == reqwest::StatusCode::PRECONDITION_FAILED
            || resp.status() == reqwest::StatusCode::CONFLICT
        {
            return Ok(PutOutcome::PreconditionFailed);
        }
        check(resp, "PUT", key).await?;
        Ok(PutOutcome::Ok)
    }

    /// Probe conditional-write support with a throwaway key: create it, then
    /// PUT it again with `If-Match` pointing at a stale etag. A real 412
    /// means the server enforces `If-Match` — the one conditional that backs
    /// the HEAD CAS. `If-None-Match: *` is deliberately not probed: servers
    /// like Nutstore ignore it, and an ignored create-only header is harmless
    /// for content-addressed blobs and unique manifest keys. Inconclusive
    /// probes fail open — an ignored header is harmless, a missed CAS is not.
    async fn probe_conditional_writes(&self) -> Result<bool, AppError> {
        let key = format!(".sync-probe-{}", uuid::Uuid::new_v4().simple());
        match self
            .put(&key, b"probe".to_vec(), PutCondition::IfNoneMatch)
            .await
        {
            Ok(PutOutcome::Ok) => {}
            outcome => {
                log::warn!(
                    target: "agentero::sync",
                    "WebDAV conditional-write probe inconclusive ({outcome:?}); assuming supported"
                );
                return Ok(true);
            }
        }
        let supported = match self
            .put(
                &key,
                b"probe2".to_vec(),
                PutCondition::IfMatch("\"00000000deadbeef\"".into()),
            )
            .await
        {
            Ok(PutOutcome::PreconditionFailed) => true,
            Ok(PutOutcome::Ok) => false,
            Err(e) => {
                if let Err(e) = self.delete(&key).await {
                    log::warn!(target: "agentero::sync", "probe cleanup {key}: {e}");
                }
                return Err(e);
            }
        };
        if let Err(e) = self.delete(&key).await {
            log::warn!(target: "agentero::sync", "probe cleanup {key}: {e}");
        }
        Ok(supported)
    }
}

fn method(name: &str) -> Result<reqwest::Method, AppError> {
    reqwest::Method::from_bytes(name.as_bytes()).map_err(|e| AppError::message(e.to_string()))
}

/// Percent-encode one path segment (unreserved chars pass through; `/` is
/// never part of a segment here, but encoding it keeps hostile manifest
/// paths from escaping the directory). Kept separate from the S3 client's
/// SigV4 canonical encoder: same charset today, different specs to follow.
fn encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Percent-decode a path segment: `Url::parse` hands us already-encoded
/// input, so `new` normalizes back to raw segments for symmetric encoding.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(url: &str) -> WebdavClient {
        WebdavClient::new(&SyncBackendConfig {
            backend: crate::integration::sync::config::SyncBackendKind::Webdav,
            webdav_url: url.into(),
            webdav_username: "u".into(),
            webdav_password: "p".into(),
            ..SyncBackendConfig::default()
        })
        .unwrap()
    }

    #[test]
    fn parses_origin_and_dir_segments() {
        let c = client("https://dav.jianguoyun.com/dav/agentero/");
        assert_eq!(c.origin, "https://dav.jianguoyun.com");
        assert_eq!(c.dir, vec!["dav".to_string(), "agentero".to_string()]);
        assert_eq!(
            c.url_for("HEAD"),
            "https://dav.jianguoyun.com/dav/agentero/HEAD"
        );
        assert_eq!(
            c.url_for("blobs/ab/hash"),
            "https://dav.jianguoyun.com/dav/agentero/blobs/ab/hash"
        );
    }

    #[test]
    fn server_root_url_has_no_dir_segments() {
        let c = client("https://example.com");
        assert!(c.dir.is_empty());
        assert_eq!(c.url_for("HEAD"), "https://example.com/HEAD");
    }

    #[test]
    fn port_and_ipv6_loopback_survive_origin_parsing() {
        let c = client("http://127.0.0.1:5005/agentero");
        assert_eq!(c.origin, "http://127.0.0.1:5005");
        let c = client("http://[::1]:5005/agentero");
        assert_eq!(c.origin, "http://[::1]:5005");
    }

    #[test]
    fn segments_are_percent_encoded() {
        let c = client("https://example.com/my dir");
        assert_eq!(c.url_for("a b"), "https://example.com/my%20dir/a%20b");
    }

    #[test]
    fn encode_path_segment_escapes_reserved() {
        assert_eq!(encode_path_segment("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(encode_path_segment("a b/c"), "a%20b%2Fc");
    }
}
