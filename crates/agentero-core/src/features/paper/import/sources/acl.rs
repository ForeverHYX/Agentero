//! ACL Anthology PDF derivation.
//!
//! The Translator returns ACL Anthology pages without a PDF attachment, but the
//! PDF URL is fully determined by the landing page:
//!
//!   https://aclanthology.org/2026.acl-long.1248/
//!   → https://aclanthology.org/2026.acl-long.1248.pdf

/// Derive the canonical ACL Anthology PDF URL from a paper landing page.
///
/// Landing URLs look like `https://aclanthology.org/2026.acl-long.1248/` with
/// an `YYYY.venue-type.number` slug; the PDF is the same URL plus `.pdf`.
pub fn pdf_url(url: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_pdf_from_landing_page() {
        assert_eq!(
            pdf_url("https://aclanthology.org/2026.acl-long.1248/"),
            Some("https://aclanthology.org/2026.acl-long.1248.pdf".to_string())
        );
        assert_eq!(
            pdf_url("https://aclanthology.org/2026.acl-long.1248.pdf"),
            Some("https://aclanthology.org/2026.acl-long.1248.pdf".to_string())
        );
        assert_eq!(
            pdf_url("https://www.aclanthology.org/2025.emnlp-main.42/"),
            Some("https://www.aclanthology.org/2025.emnlp-main.42.pdf".to_string())
        );
    }

    #[test]
    fn rejects_non_paper_pages() {
        assert!(pdf_url("https://aclanthology.org/venues/acl/").is_none());
        assert!(pdf_url("https://example.com/2026.acl-long.1248/").is_none());
    }
}
