//! ECVA (ECCV proceedings) PDF derivation.
//!
//! The Translator returns ECVA pages without a PDF attachment, but the PDF URL
//! is fully determined by the paper page — the page is numbered and the PDF
//! lives under `papers/` zero-padded to five digits:
//!
//!   https://www.ecva.net/papers/eccv_2024/papers_ECCV/html/4_ECCV_2024_paper.php
//!   → https://www.ecva.net/papers/eccv_2024/papers_ECCV/papers/00004.pdf

/// Derive the canonical ECVA PDF URL from a paper page.
pub fn pdf_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if !lower.contains("ecva.net/") {
        return None;
    }
    if lower.ends_with(".pdf") {
        return Some(url.trim().to_string());
    }
    let trimmed = url.trim().trim_end_matches('/');
    let html_idx = trimmed.find("/html/")?;
    let base = &trimmed[..html_idx]; // …/papers/eccv_2024/papers_ECCV
    let file = &trimmed[html_idx + "/html/".len()..]; // {N}_ECCV_2024_paper.php
    let number = file
        .split('_')
        .next()?
        .parse::<u32>()
        .ok()
        .filter(|n| *n > 0)?;
    Some(format!("{base}/papers/{number:05}.pdf"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_pdf_from_paper_page() {
        assert_eq!(
            pdf_url("https://www.ecva.net/papers/eccv_2024/papers_ECCV/html/4_ECCV_2024_paper.php"),
            Some("https://www.ecva.net/papers/eccv_2024/papers_ECCV/papers/00004.pdf".to_string())
        );
        assert_eq!(
            pdf_url(
                "https://www.ecva.net/papers/eccv_2024/papers_ECCV/html/1234_ECCV_2024_paper.php"
            ),
            Some("https://www.ecva.net/papers/eccv_2024/papers_ECCV/papers/01234.pdf".to_string())
        );
    }

    #[test]
    fn rejects_non_paper_pages() {
        assert!(pdf_url("https://www.ecva.net/papers.php").is_none());
        assert!(pdf_url(
            "https://example.com/papers/eccv_2024/papers_ECCV/html/4_ECCV_2024_paper.php"
        )
        .is_none());
    }
}
