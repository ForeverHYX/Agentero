//! Canonical URL derivation for well-known scholarly repositories.

/// Canonical arXiv preview URLs for a bare arXiv id.
pub struct ArxivUrls {
    pub pdf: String,
    pub html: String,
    pub abs: String,
}

/// Build canonical `https://arxiv.org/{pdf,html,abs}` URLs for a bare id.
/// The caller is responsible for stripping any `arXiv:` prefix and version
/// suffix beforehand (see `scholar_api::identifiers::strip_arxiv_version`).
pub fn arxiv_canonical_urls(bare_id: &str) -> ArxivUrls {
    let bare = bare_id.trim();
    ArxivUrls {
        pdf: format!("https://arxiv.org/pdf/{bare}"),
        html: format!("https://arxiv.org/html/{bare}"),
        abs: format!("https://arxiv.org/abs/{bare}"),
    }
}

/// DOI resolver landing page.
pub fn doi_landing_url(doi: &str) -> String {
    format!("https://doi.org/{doi}")
}

// ACL Anthology / USENIX / and other venue landing-page → PDF-URL derivations
// live in `features::import::sources` (one file per venue).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arxiv_canonical_urls_use_bare_id() {
        let urls = arxiv_canonical_urls("1706.03762");
        assert_eq!(urls.pdf, "https://arxiv.org/pdf/1706.03762");
        assert_eq!(urls.html, "https://arxiv.org/html/1706.03762");
        assert_eq!(urls.abs, "https://arxiv.org/abs/1706.03762");
    }

    #[test]
    fn doi_landing_url_is_https_doi_org() {
        assert_eq!(doi_landing_url("10.1/abc"), "https://doi.org/10.1/abc");
    }
}
