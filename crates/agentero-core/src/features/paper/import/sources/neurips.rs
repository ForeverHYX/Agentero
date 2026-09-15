//! NeurIPS proceedings PDF derivation.
//!
//! The Translator returns NeurIPS abstract pages without a PDF attachment, but
//! the PDF URL is fully determined by the abstract page — swap the `hash`
//! path segment for `file` and the `-Abstract` marker for `-Paper`:
//!
//!   https://proceedings.neurips.cc/paper_files/paper/2023/hash/{H}-Abstract-Conference.html
//!   → https://proceedings.neurips.cc/paper_files/paper/2023/file/{H}-Paper-Conference.pdf
//!
//! Pre-2021 pages (`…/paper/{Y}/hash/{H}-Abstract.html`) and the Datasets and
//! Benchmarks track follow the same transform.

/// Derive the canonical NeurIPS proceedings PDF URL from an abstract page.
pub fn pdf_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if !lower.contains("proceedings.neurips.cc") && !lower.contains("papers.nips.cc") {
        return None;
    }
    if lower.ends_with(".pdf") {
        return Some(url.trim().to_string());
    }
    let trimmed = url.trim().trim_end_matches('/');
    let hash_idx = trimmed.find("/hash/")?;
    let head = &trimmed[..hash_idx];
    let tail = trimmed[hash_idx + "/hash/".len()..].strip_suffix(".html")?;
    // `{H}-Abstract[-Track].html`; the hash is hex so "-Abstract" is unambiguous.
    let tail = tail.replacen("-Abstract", "-Paper", 1);
    Some(format!("{head}/file/{tail}.pdf"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_pdf_from_abstract_page() {
        assert_eq!(
            pdf_url("https://proceedings.neurips.cc/paper_files/paper/2023/hash/0001ca33ba34ce0351e4612b744b3936-Abstract-Conference.html"),
            Some("https://proceedings.neurips.cc/paper_files/paper/2023/file/0001ca33ba34ce0351e4612b744b3936-Paper-Conference.pdf".to_string())
        );
        assert_eq!(
            pdf_url("https://papers.nips.cc/paper/2020/hash/1cf0f9f6b6b1603a4d7ba2c34334d0f0-Abstract.html"),
            Some("https://papers.nips.cc/paper/2020/file/1cf0f9f6b6b1603a4d7ba2c34334d0f0-Paper.pdf".to_string())
        );
        assert_eq!(
            pdf_url("https://proceedings.neurips.cc/paper_files/paper/2023/hash/0001ca33ba34ce0351e4612b744b3936-Abstract-Datasets_and_Benchmarks_Track.html"),
            Some("https://proceedings.neurips.cc/paper_files/paper/2023/file/0001ca33ba34ce0351e4612b744b3936-Paper-Datasets_and_Benchmarks_Track.pdf".to_string())
        );
    }

    #[test]
    fn passes_through_existing_pdf() {
        assert_eq!(
            pdf_url("https://proceedings.neurips.cc/paper_files/paper/2023/file/0001ca33ba34ce0351e4612b744b3936-Paper-Conference.pdf"),
            Some("https://proceedings.neurips.cc/paper_files/paper/2023/file/0001ca33ba34ce0351e4612b744b3936-Paper-Conference.pdf".to_string())
        );
    }

    #[test]
    fn rejects_non_abstract_pages() {
        assert!(pdf_url("https://proceedings.neurips.cc/paper_files/paper/2023").is_none());
        assert!(pdf_url("https://example.com/paper/2023/hash/x-Abstract.html").is_none());
    }
}
