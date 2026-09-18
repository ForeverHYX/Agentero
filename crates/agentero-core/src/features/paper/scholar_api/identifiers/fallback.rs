//! Hedged direct-connect fallback chains used when the Translator Runtime
//! fails (#524).
//!
//! Each resolver kind maps to an ordered chain of sources (canonical first).
//! The chain runs **speculatively concurrent with staggered starts**: every
//! source is spawned up front, but source *i* sleeps `i × HEDGE_DELAY`
//! before fetching, giving higher-priority sources a head start. Results are
//! then awaited strictly in priority order — the first `Ok` with a record
//! wins and the remaining in-flight requests are aborted. This keeps the
//! happy path at exactly one request (the primary usually answers inside its
//! head-start window) while a slow or rate-limited primary costs
//! `max(prefix latencies)` instead of the sequential `sum`.
//!
//! Error semantics at the boundary (`AppError` codes): an empty-but-successful
//! source read and a 404 both surface as `not_found`; a chain where every
//! source was rate limited surfaces as `rate_limited` (after one 2s retry
//! pass); cancellation surfaces as `cancelled`.

use std::time::Duration;

use crate::error::AppError;
use crate::features::scholar_api::client;
use crate::features::scholar_api::sources::{
    alphaxiv::AlphaxivApi, arxiv::ArxivApi, crossref::CrossrefApi, openalex::OpenAlexApi,
    pubmed::PubMedApi, semantic_scholar::SemanticScholarApi,
};
use crate::features::scholar_api::traits::AcademicApi;
use crate::features::scholar_api::{ApiError, ApiPaper, ApiQuery};

use super::kind::{ResolvedIdentifier, ARXIV, DOI, PMID};
use super::resolver;

/// Head start granted to each source over the next one in the chain. Small
/// enough that a fast failover (429s answer quickly) stays snappy, large
/// enough that a healthy primary usually answers before the backup fires.
const HEDGE_DELAY: Duration = Duration::from_millis(600);
/// Cooldown before the single retry pass when a pass saw ≥1 rate limit and
/// no success (mirrors S2's backoff floor in `semantic_scholar.rs`).
const RATE_LIMIT_COOLDOWN: Duration = Duration::from_secs(2);
/// First pass plus at most one retry pass.
const MAX_PASSES: usize = 2;

/// Sequential-priority fallback chain per resolver kind. First success wins.
const ARXIV_CHAIN: &[&'static dyn AcademicApi] = &[&ArxivApi, &SemanticScholarApi, &AlphaxivApi];
const DOI_CHAIN: &[&'static dyn AcademicApi] = &[&CrossrefApi, &SemanticScholarApi, &OpenAlexApi];
const PMID_CHAIN: &[&'static dyn AcademicApi] = &[&PubMedApi];

fn chain_for_kind(kind: &str) -> Option<&'static [&'static dyn AcademicApi]> {
    match kind {
        ARXIV => Some(ARXIV_CHAIN),
        DOI => Some(DOI_CHAIN),
        PMID => Some(PMID_CHAIN),
        _ => None,
    }
}

/// Which error a fully-failed chain pass should surface.
/// Preference: RateLimited > NotFound > first collected error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FinalError {
    RateLimited,
    NotFound,
    First,
}

/// What the walker does after a pass completes without a success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChainDecision {
    /// The pass saw ≥1 rate limit and no success: sleep
    /// [`RATE_LIMIT_COOLDOWN`], then run every source once more.
    RetryPass,
    /// Give up and surface the error selected by [`FinalError`].
    Stop(FinalError),
}

/// Pure pass decision from one pass's collected `(source, error)` pairs.
/// Classification uses `matches!` because `ApiError` carries non-comparable
/// payloads; cancellation aborts the walker directly and never reaches here.
fn decide_pass(errors: &[(&'static str, ApiError)], pass: usize) -> ChainDecision {
    let rate_limited = errors
        .iter()
        .any(|(_, e)| matches!(e, ApiError::RateLimited));
    if rate_limited && pass + 1 < MAX_PASSES {
        return ChainDecision::RetryPass;
    }
    let which = if rate_limited {
        FinalError::RateLimited
    } else if errors.iter().any(|(_, e)| matches!(e, ApiError::NotFound)) {
        FinalError::NotFound
    } else {
        FinalError::First
    };
    ChainDecision::Stop(which)
}

/// Map a [`FinalError`] onto the concrete error to return.
fn final_api_error(errors: &[(&'static str, ApiError)], which: FinalError) -> ApiError {
    match which {
        FinalError::RateLimited => ApiError::RateLimited,
        FinalError::NotFound => ApiError::NotFound,
        FinalError::First => errors
            .iter()
            .next()
            .map(|(_, e)| e.clone())
            .unwrap_or(ApiError::NotFound), // unreachable: never called with no errors
    }
}

/// Run one fallback chain: spawn every source with a staggered start, await
/// in priority order, first non-empty `Ok` wins and aborts the stragglers.
async fn run_chain(
    chain: &'static [&'static dyn AcademicApi],
    query: &ApiQuery,
    task_id: Option<&str>,
) -> Result<ApiPaper, AppError> {
    for pass in 0..MAX_PASSES {
        let mut handles = Vec::with_capacity(chain.len());
        for (i, source) in chain.iter().enumerate() {
            let api_query = query.clone();
            let task = task_id.map(str::to_string);
            handles.push(tokio::spawn(async move {
                if i > 0 {
                    tokio::time::sleep(HEDGE_DELAY * i as u32).await;
                }
                client::check_cancelled(task.as_deref())?;
                source.fetch(&api_query).await
            }));
        }

        let mut errors: Vec<(&'static str, ApiError)> = Vec::new();
        for i in 0..handles.len() {
            let name = chain[i].name();
            // `&mut JoinHandle` awaits in place, keeping un-awaited handles
            // alive so they can be aborted on an early win.
            match (&mut handles[i]).await {
                Ok(Ok(papers)) if !papers.is_empty() => {
                    abort_stragglers(&handles[i + 1..]);
                    return Ok(papers.into_iter().next().expect("non-empty checked"));
                }
                // Ok(empty) is the trait's "no record" contract.
                Ok(Ok(_)) => errors.push((name, ApiError::NotFound)),
                Ok(Err(ApiError::Cancelled)) => {
                    abort_stragglers(&handles[i + 1..]);
                    return Err(ApiError::Cancelled.into());
                }
                Ok(Err(e)) => {
                    log::warn!(
                        target: "agentero::lookup",
                        "fallback chain for {} via {name} failed ({e}); trying next source",
                        query.kind()
                    );
                    errors.push((name, e));
                }
                Err(_) => errors.push((name, ApiError::Network(format!("{name} task failed")))),
            }
        }

        match decide_pass(&errors, pass) {
            ChainDecision::RetryPass => {
                log::warn!(
                    target: "agentero::lookup",
                    "fallback chain for {} produced no hit and saw a rate limit; retrying once in {:?}",
                    query.kind(),
                    RATE_LIMIT_COOLDOWN
                );
                tokio::time::sleep(RATE_LIMIT_COOLDOWN).await;
            }
            ChainDecision::Stop(which) => {
                return Err(final_api_error(&errors, which).into());
            }
        }
    }
    unreachable!("decide_pass returns Stop on the last pass")
}

fn abort_stragglers(handles: &[tokio::task::JoinHandle<Result<Vec<ApiPaper>, ApiError>>]) {
    for handle in handles {
        handle.abort();
    }
}

/// Resolve an arXiv id through the arXiv chain (Atom → S2 → alphaXiv).
pub async fn fetch_arxiv_metadata(
    arxiv_id: &str,
    task_id: Option<&str>,
) -> Result<ApiPaper, AppError> {
    run_chain(
        ARXIV_CHAIN,
        &ApiQuery::ArxivId(arxiv_id.to_string()),
        task_id,
    )
    .await
}

/// First fallback chain whose resolver matches `text`, probing the table
/// independently of the primary identifier: a `doi.org` URL resolves as
/// `url` first, yet its DOI still gets the Crossref chain.
pub async fn fetch_direct_fallback(
    text: &str,
    task_id: Option<&str>,
) -> Option<Result<ApiPaper, AppError>> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    for resolver in resolver::resolvers() {
        let Some(ResolvedIdentifier { kind, value, .. }) = resolver.extract(t) else {
            continue;
        };
        let query = match kind {
            ARXIV => ApiQuery::ArxivId(value),
            DOI => ApiQuery::Doi(value),
            PMID => ApiQuery::Pmid(value),
            // ISBN / URL / ADS have no direct-connect chain yet; keep
            // probing the next resolver (a doi.org URL matches `url` first,
            // then the DOI resolver extracts its embedded DOI).
            _ => continue,
        };
        let chain = chain_for_kind(kind).expect("kind with a query has a chain");
        return Some(run_chain(chain, &query, task_id).await);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::kind::{ADS, ISBN, URL};
    use super::*;

    #[test]
    fn fallback_chains_match_resolver_kinds() {
        let names = |c: &[&'static dyn AcademicApi]| c.iter().map(|s| s.name()).collect::<Vec<_>>();
        assert_eq!(names(ARXIV_CHAIN), vec!["arxiv", "s2", "alphaxiv"]);
        assert_eq!(names(DOI_CHAIN), vec!["crossref", "s2", "openalex"]);
        assert_eq!(names(PMID_CHAIN), vec!["pubmed"]);
        for kind in [ARXIV, DOI, PMID] {
            assert!(chain_for_kind(kind).is_some(), "{kind} needs a chain");
        }
        for kind in [URL, ISBN, ADS] {
            assert!(
                chain_for_kind(kind).is_none(),
                "{kind} must stay no-fallback"
            );
        }
    }

    #[test]
    fn retry_pass_when_any_rate_limited_on_first_pass() {
        let errors = vec![("arxiv", ApiError::RateLimited), ("s2", ApiError::NotFound)];
        assert_eq!(decide_pass(&errors, 0), ChainDecision::RetryPass);
    }

    #[test]
    fn no_retry_when_nothing_rate_limited() {
        let errors = vec![("arxiv", ApiError::NotFound), ("s2", ApiError::NotFound)];
        assert_eq!(
            decide_pass(&errors, 0),
            ChainDecision::Stop(FinalError::NotFound)
        );
    }

    #[test]
    fn no_retry_on_second_pass_even_if_rate_limited() {
        let errors = vec![("arxiv", ApiError::RateLimited)];
        assert_eq!(
            decide_pass(&errors, 1),
            ChainDecision::Stop(FinalError::RateLimited)
        );
    }

    #[test]
    fn not_found_preferred_over_first_network_error() {
        let errors = vec![
            ("s2", ApiError::Network("timeout".into())),
            ("alphaxiv", ApiError::NotFound),
        ];
        assert_eq!(
            decide_pass(&errors, 0),
            ChainDecision::Stop(FinalError::NotFound)
        );
    }

    #[test]
    fn first_error_when_no_not_found() {
        let errors = vec![
            ("arxiv", ApiError::Network("x".into())),
            ("s2", ApiError::Parse("y".into())),
        ];
        assert_eq!(
            decide_pass(&errors, 0),
            ChainDecision::Stop(FinalError::First)
        );
    }

    #[test]
    fn final_error_prefers_rate_limited() {
        let errors = vec![("arxiv", ApiError::NotFound), ("s2", ApiError::RateLimited)];
        assert!(matches!(
            final_api_error(&errors, FinalError::RateLimited),
            ApiError::RateLimited
        ));
    }

    #[test]
    fn final_error_first_returns_first() {
        let errors = vec![("arxiv", ApiError::Network("boom".into()))];
        match final_api_error(&errors, FinalError::First) {
            ApiError::Network(msg) => assert_eq!(msg, "boom"),
            other => panic!("expected network error, got {other:?}"),
        }
    }
}
