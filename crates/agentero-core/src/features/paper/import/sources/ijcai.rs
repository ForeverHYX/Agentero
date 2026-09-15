//! IJCAI proceedings PDF derivation.
//!
//! The Translator returns IJCAI paper pages without a PDF attachment, but the
//! PDF URL is fully determined by the paper number — zero-pad it to four
//! digits and append `.pdf`:
//!
//!   https://www.ijcai.org/proceedings/2025/1
//!   → https://www.ijcai.org/proceedings/2025/0001.pdf

/// Derive the canonical IJCAI proceedings PDF URL from a paper page.
pub fn pdf_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if !lower.contains("ijcai.org/proceedings/") {
        return None;
    }
    if lower.ends_with(".pdf") {
        return Some(url.trim().to_string());
    }
    let trimmed = url.trim().trim_end_matches('/');
    let parts: Vec<&str> = trimmed.split('/').collect();
    // [https:, "", host, proceedings, {YYYY}, {paper-number}]
    if parts.len() != 6 {
        return None;
    }
    let year = parts[4];
    if year.len() != 4 || !year.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let number = parts[5].parse::<u32>().ok()?;
    Some(format!(
        "https://www.ijcai.org/proceedings/{year}/{number:04}.pdf"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_pdf_from_paper_page() {
        assert_eq!(
            pdf_url("https://www.ijcai.org/proceedings/2025/1"),
            Some("https://www.ijcai.org/proceedings/2025/0001.pdf".to_string())
        );
        assert_eq!(
            pdf_url("https://www.ijcai.org/proceedings/2024/738"),
            Some("https://www.ijcai.org/proceedings/2024/0738.pdf".to_string())
        );
    }

    #[test]
    fn passes_through_existing_pdf() {
        assert_eq!(
            pdf_url("https://www.ijcai.org/proceedings/2024/0123.pdf"),
            Some("https://www.ijcai.org/proceedings/2024/0123.pdf".to_string())
        );
    }

    #[test]
    fn rejects_non_paper_pages() {
        assert!(pdf_url("https://www.ijcai.org/proceedings/2024").is_none());
        assert!(pdf_url("https://example.org/proceedings/2024/1").is_none());
    }
}
