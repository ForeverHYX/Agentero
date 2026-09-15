//! OpenReview PDF derivation (ICLR / COLM / TMLR / …).
//!
//! The Translator refuses OpenReview (the site serves a bot challenge to
//! server-side fetchers), but the PDF URL is fully determined by the forum
//! page — same id, `forum` becomes `pdf`:
//!
//!   https://openreview.net/forum?id=KS8mIvetg2
//!   → https://openreview.net/pdf?id=KS8mIvetg2
//!
//! Note: the same bot challenge may also block plain HTTP downloads of the
//! derived URL; the URL itself is still the canonical one to record.

/// Derive the canonical OpenReview PDF URL from a forum page.
pub fn pdf_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if !lower.contains("openreview.net/") {
        return None;
    }
    if lower.contains("/pdf?id=") || lower.ends_with("/pdf") {
        return Some(url.trim().to_string());
    }
    let trimmed = url.trim();
    if !trimmed.contains("/forum?id=") {
        return None;
    }
    Some(trimmed.replacen("/forum?id=", "/pdf?id=", 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_pdf_from_forum() {
        assert_eq!(
            pdf_url("https://openreview.net/forum?id=KS8mIvetg2"),
            Some("https://openreview.net/pdf?id=KS8mIvetg2".to_string())
        );
    }

    #[test]
    fn passes_through_existing_pdf() {
        assert_eq!(
            pdf_url("https://openreview.net/pdf?id=KS8mIvetg2"),
            Some("https://openreview.net/pdf?id=KS8mIvetg2".to_string())
        );
    }

    #[test]
    fn rejects_non_forum_pages() {
        assert!(pdf_url("https://openreview.net/group?id=ICLR.cc/2024/Conference").is_none());
        assert!(pdf_url("https://example.com/forum?id=x").is_none());
    }
}
