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

/// Derive the canonical ACL Anthology PDF URL from a paper landing page.
/// ACL Anthology paper URLs look like:
///   https://aclanthology.org/2026.acl-long.1248/
/// and the PDF is always:
///   https://aclanthology.org/2026.acl-long.1248.pdf
pub fn acl_anthology_pdf_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if !lower.contains("aclanthology.org/") {
        return None;
    }
    // Already a PDF.
    if lower.ends_with(".pdf") {
        return Some(url.trim().to_string());
    }
    let trimmed = url.trim_end_matches('/');
    let slug = trimmed.rsplit('/').next()?;
    // Expect: YYYY.venue-type.number (e.g. 2026.acl-long.1248)
    let parts: Vec<&str> = slug.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    if parts[0].len() != 4 || !parts[0].chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !parts[1].contains('-') {
        return None;
    }
    if !parts[2].chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("{}.pdf", trimmed))
}

/// Derive the canonical USENIX presentation PDF URL from a paper presentation page.
/// USENIX presentation URLs look like:
///   https://www.usenix.org/conference/atc24/presentation/liu-qingyuan
/// and the PDF is always:
///   https://www.usenix.org/system/files/atc24-liu-qingyuan.pdf
pub fn usenix_presentation_pdf_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if !lower.contains("usenix.org/conference/") || !lower.contains("/presentation/") {
        return None;
    }
    if lower.ends_with(".pdf") {
        return Some(url.trim().to_string());
    }
    let trimmed = url.trim().trim_end_matches('/');
    let parts: Vec<&str> = trimmed.split('/').collect();
    let conf_idx = parts
        .iter()
        .position(|&p| p.eq_ignore_ascii_case("conference"))?;
    let conf = parts.get(conf_idx + 1)?;
    let pres_idx = parts
        .iter()
        .position(|&p| p.eq_ignore_ascii_case("presentation"))?;
    let slug = parts.get(pres_idx + 1)?;
    if conf.is_empty() || slug.is_empty() {
        return None;
    }
    Some(format!(
        "https://www.usenix.org/system/files/{conf}-{slug}.pdf"
    ))
}

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

    #[test]
    fn acl_anthology_pdf_url_derivation() {
        assert_eq!(
            acl_anthology_pdf_url("https://aclanthology.org/2026.acl-long.1248/"),
            Some("https://aclanthology.org/2026.acl-long.1248.pdf".to_string())
        );
        assert_eq!(
            acl_anthology_pdf_url("https://aclanthology.org/2026.acl-long.1248.pdf"),
            Some("https://aclanthology.org/2026.acl-long.1248.pdf".to_string())
        );
        assert_eq!(
            acl_anthology_pdf_url("https://www.aclanthology.org/2025.emnlp-main.42/"),
            Some("https://www.aclanthology.org/2025.emnlp-main.42.pdf".to_string())
        );
        assert!(acl_anthology_pdf_url("https://aclanthology.org/venues/acl/").is_none());
        assert!(acl_anthology_pdf_url("https://example.com/2026.acl-long.1248/").is_none());
    }

    #[test]
    fn usenix_presentation_pdf_url_derivation() {
        assert_eq!(
            usenix_presentation_pdf_url(
                "https://www.usenix.org/conference/atc24/presentation/liu-qingyuan"
            ),
            Some("https://www.usenix.org/system/files/atc24-liu-qingyuan.pdf".to_string())
        );
        assert_eq!(
            usenix_presentation_pdf_url(
                "https://www.usenix.org/conference/osdi24/presentation/chen"
            ),
            Some("https://www.usenix.org/system/files/osdi24-chen.pdf".to_string())
        );
        assert_eq!(
            usenix_presentation_pdf_url(
                "https://www.usenix.org/system/files/atc24-liu-qingyuan.pdf"
            ),
            None
        );
        assert_eq!(
            usenix_presentation_pdf_url("https://www.usenix.org/conference/atc24"),
            None
        );
    }
}
