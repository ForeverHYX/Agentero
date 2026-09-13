//! alphaXiv public JSON API source.
//!
//! Undocumented public endpoints (no auth): `GET /papers/v3/{id}/preview`
//! and `GET /v1/search/paper?q=…`. alphaXiv is an arXiv mirror/discovery
//! layer, not a bibliographic authority — treat it as a best-effort backup
//! source, never a primary one; every failure degrades to the next source
//! in the chain. (Same posture as the Zotero recognizer endpoint, see
//! `src-tauri/src/features/paper/import/recognize/pdf_recognize.rs`.)

use async_trait::async_trait;
use serde_json::Value;

use crate::features::scholar_api::client;
use crate::features::scholar_api::identifiers::strip_arxiv_version;
use crate::features::scholar_api::traits::AcademicApi;
use crate::features::scholar_api::{
    ApiCapability, ApiError, ApiPaper, ApiQuery, PaperIdentifiers, PaperUrls,
};

const SOURCE: &str = "alphaxiv";
const API_BASE: &str = "https://api.alphaxiv.org";

/// alphaXiv metadata source (best-effort backup; see module docs).
#[derive(Debug, Clone, Default)]
pub struct AlphaxivApi;

#[async_trait]
impl AcademicApi for AlphaxivApi {
    fn name(&self) -> &'static str {
        SOURCE
    }

    fn capabilities(&self) -> ApiCapability {
        ApiCapability::FETCH_BY_ARXIV
            | ApiCapability::SEARCH_BY_TITLE
            | ApiCapability::PROVIDE_ABSTRACT
    }

    fn priority(&self) -> i32 {
        50
    }

    async fn fetch(&self, query: &ApiQuery) -> Result<Vec<ApiPaper>, ApiError> {
        match query {
            ApiQuery::ArxivId(id) => fetch_by_id(id).await.map(|p| vec![p]),
            ApiQuery::Title(title) => search_by_title(title, 5).await,
            _ => Err(ApiError::UnsupportedQuery(query.clone())),
        }
    }
}

async fn fetch_by_id(id: &str) -> Result<ApiPaper, ApiError> {
    let bare = strip_arxiv_version(id);
    let url = format!(
        "{API_BASE}/papers/v3/{}/preview",
        urlencoding::encode(&bare)
    );
    let value = client::get_json(&url).await?;
    map_paper(&value).ok_or(ApiError::NotFound)
}

async fn search_by_title(title: &str, limit: usize) -> Result<Vec<ApiPaper>, ApiError> {
    let url = format!(
        "{API_BASE}/v1/search/paper?q={}",
        urlencoding::encode(title)
    );
    let value = client::get_json(&url).await?;
    Ok(parse_search_results(&value, limit))
}

/// Top-level JSON array of preview-shaped items (same mapper as by-id).
fn parse_search_results(value: &Value, limit: usize) -> Vec<ApiPaper> {
    value
        .as_array()
        .map(|items| items.iter().filter_map(map_paper).take(limit).collect())
        .unwrap_or_default()
}

/// Shared mapper for the preview/search item shape. `None` when no title.
fn map_paper(item: &Value) -> Option<ApiPaper> {
    let title = item
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|t| !t.is_empty())?
        .to_string();

    // Bare arXiv id (canonical_id carries the version suffix instead).
    let arxiv_id = item
        .get("universal_paper_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);

    let authors: Vec<String> = item
        .get("authors")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.as_str())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    // ISO 8601, e.g. "2023-08-02T07:41:18.000Z".
    let publication_date = item
        .get("publication_date")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let year = publication_date
        .and_then(|d| d.get(..4))
        .and_then(|y| y.parse::<i32>().ok());
    let date = publication_date
        .and_then(|d| d.get(..10))
        .map(String::from)
        .filter(|s| !s.is_empty());

    // Only the real abstract — `paper_summary` is alphaXiv's AI-generated
    // summary and must not pollute the bibliographic record.
    let abstract_text = item
        .get("abstract")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);

    let urls = arxiv_id.as_deref().map(|id| PaperUrls {
        pdf: Some(format!("https://arxiv.org/pdf/{id}")),
        html: None,
        landing: Some(format!("https://arxiv.org/abs/{id}")),
    });

    Some(ApiPaper {
        identifiers: PaperIdentifiers {
            doi: None,
            arxiv_id,
            isbn: None,
            pmid: None,
        },
        title,
        authors,
        year,
        date,
        venue: None,
        volume: None,
        issue: None,
        pages: None,
        publisher: None,
        abstract_text,
        urls: urls.unwrap_or_default(),
        // Vote/visit metrics are popularity signals, not citations.
        citation_count: None,
        language: None,
        source: SOURCE,
        raw: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn preview_fixture() -> Value {
        json!({
            "id": "0189b531-a930-7613-9d2e-dd918c8435a5",
            "title": "Attention Is All You Need",
            "abstract": "The dominant sequence transduction models are based on complex recurrent networks.",
            "universal_paper_id": "1706.03762",
            "canonical_id": "1706.03762v7",
            "publication_date": "2023-08-02T07:41:18.000Z",
            "authors": [
                "Ashish Vaswani",
                "Noam Shazeer",
                "Niki Parmar",
                "Jakob Uszkoreit",
                "Llion Jones",
                "Aidan N. Gomez",
                "Lukasz Kaiser",
                "Illia Polosukhin"
            ],
            "paper_summary": { "summary": "AI-generated summary that must be ignored" },
            "metrics": { "upvotes_count": 1121 }
        })
    }

    #[test]
    fn maps_preview_object() {
        let paper = map_paper(&preview_fixture()).expect("mapped");
        assert_eq!(paper.title, "Attention Is All You Need");
        // Bare id, not the versioned canonical_id.
        assert_eq!(paper.identifiers.arxiv_id.as_deref(), Some("1706.03762"));
        assert_eq!(paper.year, Some(2023));
        assert_eq!(paper.date.as_deref(), Some("2023-08-02"));
        assert_eq!(paper.authors.len(), 8);
        assert_eq!(paper.authors[0], "Ashish Vaswani");
        // Real abstract only; the AI summary must not leak in.
        assert!(paper
            .abstract_text
            .as_deref()
            .is_some_and(|a| a.starts_with("The dominant sequence")));
        assert_eq!(
            paper.urls.landing.as_deref(),
            Some("https://arxiv.org/abs/1706.03762")
        );
        assert_eq!(
            paper.urls.pdf.as_deref(),
            Some("https://arxiv.org/pdf/1706.03762")
        );
        assert_eq!(paper.citation_count, None);
        assert_eq!(paper.source, "alphaxiv");
    }

    #[test]
    fn maps_search_array_with_limit() {
        let first = preview_fixture();
        let second = json!({
            "title": "Forget Attention: Importance-Aware Attention",
            "universal_paper_id": "2606.02332",
            "publication_date": "2026-06-02T05:51:37.000Z",
            "authors": ["Ada Lovelace"]
        });
        let results = parse_search_results(&json!([first, second]), 5);
        assert_eq!(results.len(), 2);
        assert_eq!(results[1].year, Some(2026));

        let truncated = parse_search_results(&json!([first, second]), 1);
        assert_eq!(truncated.len(), 1);
    }

    #[test]
    fn missing_title_returns_none() {
        assert_eq!(map_paper(&json!({})), None);
        assert_eq!(
            map_paper(&json!({ "universal_paper_id": "1706.03762" })),
            None
        );
    }

    #[test]
    fn missing_optionals_default_to_empty() {
        let paper = map_paper(&json!({ "title": "Untitled Study" })).expect("mapped");
        assert!(paper.authors.is_empty());
        assert_eq!(paper.year, None);
        assert_eq!(paper.date, None);
        assert_eq!(paper.abstract_text, None);
        assert_eq!(paper.urls.landing, None);
    }
}
