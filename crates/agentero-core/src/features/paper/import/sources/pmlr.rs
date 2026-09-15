//! PMLR proceedings PDF derivation (ICML / AISTATS / UAI / …).
//!
//! The Translator returns PMLR abstract pages without a PDF attachment, but
//! the PDF URL is fully determined by the volume and paper key. The host moved
//! its PDFs at v228 (ICML 2024, probed 2026-09): earlier volumes serve them on
//! the same domain, later ones from the `mlresearch` GitHub mirror that the
//! site itself publishes as `citation_pdf_url`.
//!
//!   https://proceedings.mlr.press/v70/achab17a.html
//!   → https://proceedings.mlr.press/v70/achab17a/achab17a.pdf
//!   https://proceedings.mlr.press/v235/abad-rocamora24a.html
//!   → https://raw.githubusercontent.com/mlresearch/v235/main/assets/abad-rocamora24a/abad-rocamora24a.pdf

/// First volume whose PDFs live on the GitHub mirror instead of mlr.press.
const GITHUB_MIRROR_MIN_VOLUME: u32 = 228;

/// Derive the canonical PMLR PDF URL from an abstract page.
pub fn pdf_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if !lower.contains("proceedings.mlr.press/") {
        return None;
    }
    if lower.ends_with(".pdf") {
        return Some(url.trim().to_string());
    }
    let trimmed = url.trim().trim_end_matches('/');
    let parts: Vec<&str> = trimmed.split('/').collect();
    // [https:, "", host, v{volume}, {key}.html]
    if parts.len() != 5 {
        return None;
    }
    let volume: u32 = parts[3].strip_prefix('v')?.parse().ok()?;
    let key = parts[4].strip_suffix(".html")?;
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    if volume >= GITHUB_MIRROR_MIN_VOLUME {
        Some(format!(
            "https://raw.githubusercontent.com/mlresearch/v{volume}/main/assets/{key}/{key}.pdf"
        ))
    } else {
        Some(format!(
            "https://proceedings.mlr.press/v{volume}/{key}/{key}.pdf"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_same_domain_pdf_for_pre_v228() {
        assert_eq!(
            pdf_url("https://proceedings.mlr.press/v70/achab17a.html"),
            Some("https://proceedings.mlr.press/v70/achab17a/achab17a.pdf".to_string())
        );
        assert_eq!(
            pdf_url("https://proceedings.mlr.press/v216/abu-el-haija23a.html"),
            Some(
                "https://proceedings.mlr.press/v216/abu-el-haija23a/abu-el-haija23a.pdf"
                    .to_string()
            )
        );
    }

    #[test]
    fn derives_github_mirror_pdf_from_v228() {
        assert_eq!(
            pdf_url("https://proceedings.mlr.press/v228/sanborn24a.html"),
            Some("https://raw.githubusercontent.com/mlresearch/v228/main/assets/sanborn24a/sanborn24a.pdf".to_string())
        );
        assert_eq!(
            pdf_url("https://proceedings.mlr.press/v235/abad-rocamora24a.html"),
            Some("https://raw.githubusercontent.com/mlresearch/v235/main/assets/abad-rocamora24a/abad-rocamora24a.pdf".to_string())
        );
    }

    #[test]
    fn rejects_non_paper_pages() {
        assert!(pdf_url("https://proceedings.mlr.press/v235/").is_none());
        assert!(pdf_url("https://example.com/v235/a.html").is_none());
    }
}
