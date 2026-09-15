//! USENIX presentation PDF derivation.
//!
//! The Translator returns USENIX presentation pages without a PDF attachment,
//! but the PDF URL is fully determined by the presentation page:
//!
//!   https://www.usenix.org/conference/atc24/presentation/liu-qingyuan
//!   → https://www.usenix.org/system/files/atc24-liu-qingyuan.pdf

/// Derive the canonical USENIX presentation PDF URL from a paper page.
pub fn pdf_url(url: &str) -> Option<String> {
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
    fn derives_pdf_from_presentation_page() {
        assert_eq!(
            pdf_url("https://www.usenix.org/conference/atc24/presentation/liu-qingyuan"),
            Some("https://www.usenix.org/system/files/atc24-liu-qingyuan.pdf".to_string())
        );
        assert_eq!(
            pdf_url("https://www.usenix.org/conference/osdi24/presentation/chen"),
            Some("https://www.usenix.org/system/files/osdi24-chen.pdf".to_string())
        );
    }

    #[test]
    fn rejects_non_presentation_pages() {
        assert_eq!(
            pdf_url("https://www.usenix.org/system/files/atc24-liu-qingyuan.pdf"),
            None
        );
        assert_eq!(pdf_url("https://www.usenix.org/conference/atc24"), None);
    }
}
