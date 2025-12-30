//! Server-side fuzzy title ranking.
//!
//! Ferrex uses PostgreSQL for *candidate retrieval* (fast indexed filtering), then applies a
//! skim/fzf-like scorer in Rust to produce intuitive relevance ordering without relying on
//! Postgres' global `pg_trgm.similarity_threshold`.
//!
//! This is intentionally scoped to title-only search for now; adding additional fields should
//! extend candidate retrieval and feed a combined "searchable text" into the matcher.

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use crate::player_prelude::MediaID;
use crate::query::types::SearchField;
use crate::query::types::SearchQuery;

#[derive(Debug, Clone)]
pub(crate) struct TitleCandidate {
    pub(crate) media_id: MediaID,
    pub(crate) title: String,
}

#[derive(Debug, Clone)]
pub(crate) struct RankedTitleCandidate {
    pub(crate) media_id: MediaID,
    pub(crate) title: String,
    pub(crate) title_lower: String,
    pub(crate) score: i64,
}

pub(crate) fn supports_title_only_search(search: &SearchQuery) -> bool {
    matches!(search.fields.as_slice(), [SearchField::Title])
}

pub(crate) fn rank_title_candidates(
    query: &str,
    candidates: impl IntoIterator<Item = TitleCandidate>,
) -> Vec<RankedTitleCandidate> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }

    let query_lower = query.to_lowercase();
    let matcher = SkimMatcherV2::default().ignore_case();

    let mut ranked = Vec::new();

    for candidate in candidates {
        let title_lower = candidate.title.to_lowercase();
        let Some(base_score) = matcher.fuzzy_match(&candidate.title, query)
        else {
            continue;
        };

        let score = apply_match_bonuses(
            base_score,
            &title_lower,
            &candidate.title,
            query,
            &query_lower,
        );

        ranked.push(RankedTitleCandidate {
            media_id: candidate.media_id,
            title: candidate.title,
            title_lower,
            score,
        });
    }

    ranked.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.title.len().cmp(&b.title.len()))
            .then_with(|| a.title_lower.cmp(&b.title_lower))
    });

    ranked
}

fn apply_match_bonuses(
    base_score: i64,
    title_lower: &str,
    title: &str,
    query: &str,
    query_lower: &str,
) -> i64 {
    let mut score = base_score;

    if title_lower == query_lower {
        score = score.saturating_add(100_000);
        return score;
    }

    if title_lower.starts_with(query_lower) {
        score = score.saturating_add(25_000);
    }

    if let Some(position) = title_lower.find(query_lower) {
        score = score.saturating_add(10_000);
        score = score.saturating_add(2_000_i64.saturating_sub(position as i64));
    }

    if title_lower
        .split_whitespace()
        .any(|word| word.starts_with(query_lower))
    {
        score = score.saturating_add(5_000);
    }

    score = score.saturating_add(exact_token_bonus(title_lower, query));

    // Mild length penalty so that shorter, tighter titles win ties.
    score = score.saturating_sub(title.len().min(200) as i64);

    score
}

fn exact_token_bonus(title_lower: &str, query: &str) -> i64 {
    let mut bonus = 0_i64;
    for token in query
        .split_whitespace()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
    {
        let token_lower = token.to_lowercase();
        if title_lower.split_whitespace().any(|w| w == token_lower) {
            bonus = bonus.saturating_add(1_000);
        }
    }
    bonus
}
