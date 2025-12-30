use std::cmp::Ordering;
use std::collections::HashSet;

use chrono::Datelike;
use ordered_float::NotNan;
use tmdb_api::{movie::MovieShort, tvshow::TVShowShort};

use crate::domain::scan::orchestration::series::collapse_whitespace;

const TITLE_ACCEPT_MIN_OVERLAP_BP: u16 = 650;
const TITLE_ACCEPT_MIN_JACCARD_BP: u16 = 420;

#[derive(Debug, Clone)]
struct TitleKey {
    normalized: String,
    tokens: Vec<String>,
}

impl TitleKey {
    fn new(raw: &str) -> Self {
        let normalized = normalize_title(raw);
        let tokens = tokenize_title(&normalized);
        Self { normalized, tokens }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TitleMatch {
    pub exact_normalized: bool,
    pub contains_normalized: bool,
    pub overlap_bp: u16,
    pub jaccard_bp: u16,
    pub intersection_tokens: u16,
    pub query_tokens: u16,
    pub candidate_tokens: u16,
}

impl TitleMatch {
    pub fn is_acceptable(self) -> bool {
        if self.exact_normalized {
            return true;
        }

        if self.query_tokens == 0 || self.candidate_tokens == 0 {
            return false;
        }

        self.overlap_bp >= TITLE_ACCEPT_MIN_OVERLAP_BP
            && self.jaccard_bp >= TITLE_ACCEPT_MIN_JACCARD_BP
    }

    fn cmp_best(self, other: Self) -> Ordering {
        self.exact_normalized
            .cmp(&other.exact_normalized)
            .then_with(|| self.overlap_bp.cmp(&other.overlap_bp))
            .then_with(|| self.jaccard_bp.cmp(&other.jaccard_bp))
            .then_with(|| {
                self.contains_normalized.cmp(&other.contains_normalized)
            })
            .then_with(|| {
                self.intersection_tokens.cmp(&other.intersection_tokens)
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YearRank {
    NotApplicable,
    Diff(u16),
    Unknown,
}

impl YearRank {
    fn cmp_best(self, other: Self) -> Ordering {
        use YearRank::*;
        match (self, other) {
            (NotApplicable, NotApplicable) => Ordering::Equal,
            (NotApplicable, _) | (_, NotApplicable) => Ordering::Equal,
            (Diff(a), Diff(b)) => b.cmp(&a), // lower diff is better
            (Diff(_), Unknown) => Ordering::Greater,
            (Unknown, Diff(_)) => Ordering::Less,
            (Unknown, Unknown) => Ordering::Equal,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateRank {
    pub has_poster: bool,
    pub title: TitleMatch,
    pub year: YearRank,
    pub vote_count: u64,
    pub popularity: NotNan<f64>,
}

impl CandidateRank {
    pub fn is_acceptable(&self) -> bool {
        self.has_poster && self.title.is_acceptable()
    }

    fn cmp_best(&self, other: &Self) -> Ordering {
        self.has_poster
            .cmp(&other.has_poster)
            .then_with(|| self.title.cmp_best(other.title))
            .then_with(|| self.year.cmp_best(other.year))
            .then_with(|| self.vote_count.cmp(&other.vote_count))
            .then_with(|| self.popularity.cmp(&other.popularity))
    }
}

#[derive(Debug, Clone)]
pub struct RankedCandidate<'a, T> {
    pub candidate: &'a T,
    pub rank: CandidateRank,
}

fn normalize_title(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
        } else {
            out.push(' ');
        }
    }
    collapse_whitespace(&out)
}

fn is_stopword(token: &str) -> bool {
    matches!(
        token,
        "the"
            | "a"
            | "an"
            | "to"
            | "of"
            | "and"
            | "or"
            | "for"
            | "in"
            | "on"
            | "at"
            | "with"
            | "from"
            | "by"
    )
}

fn tokenize_title(normalized: &str) -> Vec<String> {
    normalized
        .split_whitespace()
        .filter_map(|token| {
            let token = token.trim();
            if token.is_empty() {
                return None;
            }
            if is_stopword(token) {
                return None;
            }
            if token.len() == 1 {
                return None;
            }
            Some(token.to_string())
        })
        .collect()
}

fn has_poster_path(poster_path: Option<&str>) -> bool {
    poster_path
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .is_some()
}

fn year_rank(query_year: Option<u16>, candidate_year: Option<u16>) -> YearRank {
    let Some(query_year) = query_year else {
        return YearRank::NotApplicable;
    };
    let Some(candidate_year) = candidate_year else {
        return YearRank::Unknown;
    };
    YearRank::Diff(query_year.abs_diff(candidate_year))
}

fn not_nan_or_zero(value: f64) -> NotNan<f64> {
    NotNan::new(value)
        .unwrap_or_else(|_| NotNan::new(0.0).expect("0 is not NaN"))
}

fn title_match(query: &TitleKey, candidate: &TitleKey) -> TitleMatch {
    let exact_normalized = query.normalized == candidate.normalized;
    let contains_normalized = candidate.normalized.contains(&query.normalized)
        || query.normalized.contains(&candidate.normalized);

    let query_tokens = query.tokens.len() as u16;
    let candidate_tokens = candidate.tokens.len() as u16;

    if query_tokens == 0 || candidate_tokens == 0 {
        return TitleMatch {
            exact_normalized,
            contains_normalized,
            overlap_bp: 0,
            jaccard_bp: 0,
            intersection_tokens: 0,
            query_tokens,
            candidate_tokens,
        };
    }

    let query_set: HashSet<&str> =
        query.tokens.iter().map(String::as_str).collect();
    let candidate_set: HashSet<&str> =
        candidate.tokens.iter().map(String::as_str).collect();

    let intersection = query_set.intersection(&candidate_set).count() as u16;
    let union = query_set.union(&candidate_set).count() as u16;

    let overlap_bp =
        ((intersection as u32) * 1000 / (query_set.len().max(1) as u32)) as u16;
    let jaccard_bp = if union == 0 {
        0
    } else {
        ((intersection as u32) * 1000 / (union as u32)) as u16
    };

    TitleMatch {
        exact_normalized,
        contains_normalized,
        overlap_bp,
        jaccard_bp,
        intersection_tokens: intersection,
        query_tokens,
        candidate_tokens,
    }
}

fn best_title_match(query: &TitleKey, candidates: &[&str]) -> TitleMatch {
    let mut best = TitleMatch {
        exact_normalized: false,
        contains_normalized: false,
        overlap_bp: 0,
        jaccard_bp: 0,
        intersection_tokens: 0,
        query_tokens: query.tokens.len() as u16,
        candidate_tokens: 0,
    };

    for raw in candidates {
        let key = TitleKey::new(raw);
        let current = title_match(query, &key);
        if current.cmp_best(best) == Ordering::Greater {
            best = current;
        }
    }

    best
}

pub fn rank_movie_candidates<'a>(
    query_title: &str,
    query_year: Option<u16>,
    results: &'a [MovieShort],
) -> Vec<RankedCandidate<'a, MovieShort>> {
    if results.is_empty() {
        return Vec::new();
    }

    let query = TitleKey::new(query_title);
    let prefer_with_poster = results.iter().any(|candidate| {
        has_poster_path(candidate.inner.poster_path.as_deref())
    });

    let mut ranked = Vec::new();
    for candidate in results {
        let has_poster =
            has_poster_path(candidate.inner.poster_path.as_deref());
        if prefer_with_poster && !has_poster {
            continue;
        }

        let candidate_year = candidate
            .inner
            .release_date
            .as_ref()
            .map(|d| d.year() as u16);

        let title = best_title_match(
            &query,
            &[
                candidate.inner.title.as_str(),
                candidate.inner.original_title.as_str(),
            ],
        );

        let rank = CandidateRank {
            has_poster,
            title,
            year: year_rank(query_year, candidate_year),
            vote_count: candidate.inner.vote_count,
            popularity: not_nan_or_zero(candidate.inner.popularity),
        };

        ranked.push(RankedCandidate { candidate, rank });
    }

    ranked.sort_by(|a, b| a.rank.cmp_best(&b.rank).reverse());
    ranked
}

pub fn rank_series_candidates<'a>(
    query_title: &str,
    query_year: Option<u16>,
    results: &'a [TVShowShort],
) -> Vec<RankedCandidate<'a, TVShowShort>> {
    if results.is_empty() {
        return Vec::new();
    }

    let query = TitleKey::new(query_title);
    let prefer_with_poster = results.iter().any(|candidate| {
        has_poster_path(candidate.inner.poster_path.as_deref())
    });

    let mut ranked = Vec::new();
    for candidate in results {
        let has_poster =
            has_poster_path(candidate.inner.poster_path.as_deref());
        if prefer_with_poster && !has_poster {
            continue;
        }

        let candidate_year = candidate
            .inner
            .first_air_date
            .as_ref()
            .map(|d| d.year() as u16);

        let title = best_title_match(
            &query,
            &[
                candidate.inner.name.as_str(),
                candidate.inner.original_name.as_str(),
            ],
        );

        let rank = CandidateRank {
            has_poster,
            title,
            year: year_rank(query_year, candidate_year),
            vote_count: candidate.inner.vote_count,
            popularity: not_nan_or_zero(candidate.inner.popularity),
        };

        ranked.push(RankedCandidate { candidate, rank });
    }

    ranked.sort_by(|a, b| a.rank.cmp_best(&b.rank).reverse());
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn movie(
        id: u64,
        title: &str,
        release_year: Option<i32>,
        poster_path: Option<&str>,
        popularity: f64,
        vote_count: u64,
    ) -> MovieShort {
        MovieShort {
            inner: tmdb_api::movie::MovieBase {
                id,
                title: title.to_string(),
                original_title: title.to_string(),
                original_language: "en".into(),
                overview: String::new(),
                release_date: release_year.map(|y| {
                    chrono::NaiveDate::from_ymd_opt(y, 1, 1)
                        .expect("valid date")
                }),
                poster_path: poster_path.map(ToString::to_string),
                backdrop_path: None,
                adult: false,
                popularity,
                vote_count,
                vote_average: 0.0,
                video: false,
            },
            genre_ids: Vec::new(),
        }
    }

    fn series(
        id: u64,
        name: &str,
        first_air_year: Option<i32>,
        poster_path: Option<&str>,
        popularity: f64,
        vote_count: u64,
    ) -> TVShowShort {
        TVShowShort {
            inner: tmdb_api::tvshow::TVShowBase {
                id,
                name: name.to_string(),
                original_name: name.to_string(),
                original_language: "en".into(),
                origin_country: Vec::new(),
                overview: None,
                first_air_date: first_air_year.map(|y| {
                    chrono::NaiveDate::from_ymd_opt(y, 1, 1)
                        .expect("valid date")
                }),
                poster_path: poster_path.map(ToString::to_string),
                backdrop_path: None,
                popularity,
                vote_count,
                vote_average: 0.0,
                adult: false,
            },
            genre_ids: Vec::new(),
        }
    }

    #[test]
    fn movie_ranking_prefers_year_when_titles_equal() {
        let results = vec![
            movie(1, "Alien", Some(1979), Some("/a.jpg"), 1.0, 10),
            movie(2, "Alien", Some(2003), Some("/b.jpg"), 999.0, 100000),
        ];

        let ranked = rank_movie_candidates("Alien", Some(1979), &results);
        assert_eq!(ranked.first().unwrap().candidate.inner.id, 1);
    }

    #[test]
    fn movie_ranking_prefers_poster_when_available() {
        let results = vec![
            movie(1, "Dune", Some(2021), None, 999.0, 100000),
            movie(2, "Dune", Some(2021), Some("/poster.jpg"), 1.0, 10),
        ];

        let ranked = rank_movie_candidates("Dune", Some(2021), &results);
        assert_eq!(ranked.first().unwrap().candidate.inner.id, 2);
    }

    #[test]
    fn series_ranking_handles_punctuation_differences() {
        let results = vec![
            series(
                10,
                "IT: Welcome to Derry",
                Some(2025),
                Some("/poster.jpg"),
                12.0,
                20,
            ),
            series(
                11,
                "IT Welcome to Derry - Behind the Scenes",
                Some(2025),
                Some("/other.jpg"),
                100.0,
                2000,
            ),
        ];

        let ranked =
            rank_series_candidates("IT Welcome to Derry", Some(2025), &results);
        assert_eq!(ranked.first().unwrap().candidate.inner.id, 10);
    }
}
