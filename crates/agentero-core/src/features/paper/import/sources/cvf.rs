//! CVF Open Access PDF derivation (CVPR / ICCV / WACV).
//!
//! The Translator returns CVF pages without a PDF attachment, but the PDF URL
//! is fully determined by the abstract page — swap the `html` path segment for
//! `papers` and `.html` for `.pdf`:
//!
//!   https://openaccess.thecvf.com/content/CVPR2024/html/Zeng_Title_CVPR_2024_paper.html
//!   → https://openaccess.thecvf.com/content/CVPR2024/papers/Zeng_Title_CVPR_2024_paper.pdf

/// Derive the canonical CVF Open Access PDF URL from an abstract page.
pub fn pdf_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if !lower.contains("openaccess.thecvf.com/") {
        return None;
    }
    if lower.ends_with(".pdf") {
        return Some(url.trim().to_string());
    }
    let trimmed = url.trim().trim_end_matches('/');
    if !trimmed.contains("/content/") || !trimmed.contains("/html/") {
        return None;
    }
    let stem = trimmed.strip_suffix(".html")?;
    if !stem.ends_with("_paper") {
        return None;
    }
    Some(format!("{}.pdf", stem.replace("/html/", "/papers/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_pdf_from_abstract_page() {
        assert_eq!(
            pdf_url("https://openaccess.thecvf.com/content/CVPR2024/html/Zeng_Unmixing_Diffusion_for_Self-Supervised_Hyperspectral_Image_Denoising_CVPR_2024_paper.html"),
            Some("https://openaccess.thecvf.com/content/CVPR2024/papers/Zeng_Unmixing_Diffusion_for_Self-Supervised_Hyperspectral_Image_Denoising_CVPR_2024_paper.pdf".to_string())
        );
        assert_eq!(
            pdf_url("https://openaccess.thecvf.com/content/ICCV2023/html/Chen_Title_ICCV_2023_paper.html"),
            Some("https://openaccess.thecvf.com/content/ICCV2023/papers/Chen_Title_ICCV_2023_paper.pdf".to_string())
        );
    }

    #[test]
    fn rejects_non_paper_pages() {
        assert!(pdf_url("https://openaccess.thecvf.com/CVPR2024?day=all").is_none());
        assert!(
            pdf_url("https://openaccess.thecvf.com/content/CVPR2024/html/author.html").is_none()
        );
        assert!(pdf_url("https://example.com/content/CVPR2024/html/a_paper.html").is_none());
    }
}
