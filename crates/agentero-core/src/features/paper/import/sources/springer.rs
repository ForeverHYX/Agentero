//! Springer Link PDF derivation (MICCAI / LNCS proceedings and journals).
//!
//! The Translator returns Springer pages without a PDF attachment, but the PDF
//! URL is fully determined by the DOI carried in the page URL:
//!
//!   https://link.springer.com/chapter/10.1007/978-3-031-83274-1_20
//!   → https://link.springer.com/content/pdf/10.1007/978-3-031-83274-1_20.pdf
//!
//! Paywalled items still resolve to a login/interstitial page instead of the
//! PDF; the derived URL is the canonical one either way.

/// Derive the canonical Springer Link PDF URL from a chapter/article page.
pub fn pdf_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if !lower.contains("link.springer.com/") {
        return None;
    }
    if lower.ends_with(".pdf") {
        return Some(url.trim().to_string());
    }
    let trimmed = url.trim().trim_end_matches('/');
    let doi = ["/chapter/", "/article/"]
        .iter()
        .filter_map(|prefix| {
            trimmed
                .find(prefix)
                .and_then(|idx| trimmed[idx + prefix.len()..].strip_prefix("10."))
                .map(|rest| format!("10.{rest}"))
        })
        .find(|doi| !doi.is_empty())?;
    Some(format!("https://link.springer.com/content/pdf/{doi}.pdf"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_pdf_from_chapter_page() {
        assert_eq!(
            pdf_url("https://link.springer.com/chapter/10.1007/978-3-031-83274-1_20"),
            Some(
                "https://link.springer.com/content/pdf/10.1007/978-3-031-83274-1_20.pdf"
                    .to_string()
            )
        );
    }

    #[test]
    fn derives_pdf_from_article_page() {
        assert_eq!(
            pdf_url("https://link.springer.com/article/10.1007/s00521-024-09999-9"),
            Some(
                "https://link.springer.com/content/pdf/10.1007/s00521-024-09999-9.pdf".to_string()
            )
        );
    }

    #[test]
    fn rejects_non_content_pages() {
        assert!(pdf_url("https://link.springer.com/search?query=MICCAI").is_none());
        assert!(pdf_url("https://link.springer.com/book/10.1007/978-3-031-71876-6").is_none());
        assert!(pdf_url("https://example.com/chapter/10.1007/x").is_none());
    }
}
