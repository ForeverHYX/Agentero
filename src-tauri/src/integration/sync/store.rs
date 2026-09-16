//! Backend-agnostic storage façade. The engine talks to this enum, not to
//! any concrete client — the surface it needs is exactly `get` (with etag)
//! and conditional `put`, both provided by S3 and WebDAV alike.

use crate::core::error::AppError;
use crate::integration::sync::config::{SyncBackendConfig, SyncBackendKind};
use crate::integration::sync::s3::S3Client;
use crate::integration::sync::webdav::WebdavClient;

pub enum PutCondition {
    /// Create-only (`If-None-Match: *`).
    IfNoneMatch,
    /// Replace-only when unchanged (`If-Match: <etag>`).
    IfMatch(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum PutOutcome {
    Ok,
    /// The conditional write lost a race (412) — caller decides how to retry.
    PreconditionFailed,
}

pub enum SyncStore {
    S3(S3Client),
    Webdav(WebdavClient),
}

impl SyncStore {
    pub fn new(cfg: &SyncBackendConfig) -> Result<Self, AppError> {
        match cfg.backend {
            SyncBackendKind::S3 => Ok(Self::S3(S3Client::new(cfg)?)),
            SyncBackendKind::Webdav => Ok(Self::Webdav(WebdavClient::new(cfg)?)),
        }
    }

    /// Verify credentials and that the store root is usable; WebDAV creates
    /// the configured directory when missing, S3 checks bucket listing.
    pub async fn ensure_root(&self) -> Result<(), AppError> {
        match self {
            Self::S3(c) => c.list("", 1).await.map(|_| ()),
            Self::Webdav(c) => c.ensure_root().await,
        }
    }

    /// GET an object. `None` on 404; otherwise `(body, etag)`.
    pub async fn get(&self, key: &str) -> Result<Option<(Vec<u8>, String)>, AppError> {
        match self {
            Self::S3(c) => c.get(key).await,
            Self::Webdav(c) => c.get(key).await,
        }
    }

    pub async fn put(
        &self,
        key: &str,
        body: Vec<u8>,
        condition: PutCondition,
    ) -> Result<PutOutcome, AppError> {
        match self {
            Self::S3(c) => c.put(key, body, condition).await,
            Self::Webdav(c) => c.put(key, body, condition).await,
        }
    }

    /// Connection-test probe: whether conditional PUTs actually enforce.
    pub async fn probe_conditional_writes(&self) -> Result<bool, AppError> {
        match self {
            Self::S3(c) => c.probe_conditional_writes().await,
            Self::Webdav(c) => c.probe_conditional_writes().await,
        }
    }
}
