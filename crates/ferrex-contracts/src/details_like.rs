//! Detail-record traits shared by owned and archived model values.
//!
//! These traits expose the subset of TMDB-style details needed by UI and
//! repository code without requiring callers to care whether a value came from
//! an owned `ferrex-model` struct or an rkyv archived snapshot.

use ferrex_model::details::{
    CastMember, CrewMember, EnhancedSeriesDetails, ExternalIds, SeasonDetails,
};

/// Read-only view of series details used by list, details, and search surfaces.
pub trait SeriesDetailsLike {
    /// Cast member reference type returned by [`SeriesDetailsLike::cast`].
    type Cast;
    /// Crew member reference type returned by [`SeriesDetailsLike::crew`].
    type Crew;
    /// External-id record type for provider ids and deep links.
    type ExIds;

    /// TMDB series id.
    fn tmdb_id(&self) -> u64;
    /// Display name for the series.
    fn name(&self) -> &str;
    /// Optional overview/summary text.
    fn overview(&self) -> Option<&str>;
    /// First-air date string when known.
    fn first_air_date(&self) -> Option<&str>;
    /// Last-air date string when known.
    fn last_air_date(&self) -> Option<&str>;
    /// Number of seasons reported by the metadata provider.
    fn num_seasons(&self) -> Option<u16>;
    /// Number of episodes reported by the metadata provider.
    fn num_episodes(&self) -> Option<u16>;
    /// Provider vote average.
    fn vote_average(&self) -> Option<f32>;
    /// Provider vote count.
    fn vote_count(&self) -> Option<u32>;
    /// Provider popularity score.
    fn popularity(&self) -> Option<f32>;
    /// Genre names.
    fn genres(&self) -> Vec<&str>;
    /// Network names.
    fn networks(&self) -> Vec<&str>;
    /// Cast entries.
    fn cast(&self) -> Vec<&Self::Cast>;
    /// Crew entries.
    fn crew(&self) -> Vec<&Self::Crew>;
    /// Provider keyword names.
    fn keywords(&self) -> Vec<&str>;
    /// External provider identifiers.
    fn external_ids(&self) -> &Self::ExIds;
}

impl SeriesDetailsLike for EnhancedSeriesDetails {
    type Cast = CastMember;
    type Crew = CrewMember;
    type ExIds = ExternalIds;

    fn tmdb_id(&self) -> u64 {
        self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn overview(&self) -> Option<&str> {
        self.overview.as_deref()
    }
    fn first_air_date(&self) -> Option<&str> {
        self.first_air_date.as_deref()
    }
    fn last_air_date(&self) -> Option<&str> {
        self.last_air_date.as_deref()
    }
    fn num_seasons(&self) -> Option<u16> {
        self.number_of_seasons
    }
    fn num_episodes(&self) -> Option<u16> {
        self.number_of_episodes
    }
    fn vote_average(&self) -> Option<f32> {
        self.vote_average
    }
    fn vote_count(&self) -> Option<u32> {
        self.vote_count
    }
    fn popularity(&self) -> Option<f32> {
        self.popularity
    }
    fn genres(&self) -> Vec<&str> {
        self.genres
            .iter()
            .map(|genre| genre.name.as_str())
            .collect()
    }
    fn networks(&self) -> Vec<&str> {
        self.networks
            .iter()
            .map(|network| network.name.as_str())
            .collect()
    }
    fn cast(&self) -> Vec<&Self::Cast> {
        self.cast.iter().collect()
    }
    fn crew(&self) -> Vec<&Self::Crew> {
        self.crew.iter().collect()
    }
    fn keywords(&self) -> Vec<&str> {
        self.keywords
            .iter()
            .map(|keyword| keyword.name.as_str())
            .collect()
    }
    fn external_ids(&self) -> &Self::ExIds {
        &self.external_ids
    }
}

/// Read-only view of season details used by season progress and navigation UI.
pub trait SeasonDetailsLike {
    /// Number of episodes in the season.
    fn num_episodes(&self) -> u16;
}

impl SeasonDetailsLike for SeasonDetails {
    fn num_episodes(&self) -> u16 {
        self.episode_count
    }
}

#[cfg(feature = "rkyv")]
mod archived {
    use super::*;
    use ferrex_model::details::{
        ArchivedCastMember, ArchivedCrewMember, ArchivedEnhancedSeriesDetails,
        ArchivedExternalIds, ArchivedSeasonDetails,
    };
    use rkyv::option::ArchivedOption;

    impl SeriesDetailsLike for ArchivedEnhancedSeriesDetails {
        type Cast = ArchivedCastMember;
        type Crew = ArchivedCrewMember;
        type ExIds = ArchivedExternalIds;

        fn tmdb_id(&self) -> u64 {
            self.id.to_native()
        }
        fn name(&self) -> &str {
            &self.name
        }
        fn overview(&self) -> Option<&str> {
            self.overview.as_deref()
        }
        fn first_air_date(&self) -> Option<&str> {
            self.first_air_date.as_deref()
        }
        fn last_air_date(&self) -> Option<&str> {
            self.last_air_date.as_deref()
        }
        fn num_seasons(&self) -> Option<u16> {
            if let ArchivedOption::Some(seasons) = self.number_of_seasons {
                Some(seasons.to_native())
            } else {
                None
            }
        }
        fn num_episodes(&self) -> Option<u16> {
            if let ArchivedOption::Some(episodes) = self.number_of_episodes {
                Some(episodes.to_native())
            } else {
                None
            }
        }
        fn vote_average(&self) -> Option<f32> {
            if let ArchivedOption::Some(vote_average) = self.vote_average {
                Some(vote_average.to_native())
            } else {
                None
            }
        }
        fn vote_count(&self) -> Option<u32> {
            if let ArchivedOption::Some(vote_count) = self.vote_count {
                Some(vote_count.to_native())
            } else {
                None
            }
        }
        fn popularity(&self) -> Option<f32> {
            if let ArchivedOption::Some(popularity) = self.popularity {
                Some(popularity.to_native())
            } else {
                None
            }
        }
        fn genres(&self) -> Vec<&str> {
            self.genres
                .iter()
                .map(|genre| genre.name.as_str())
                .collect()
        }
        fn networks(&self) -> Vec<&str> {
            self.networks
                .iter()
                .map(|network| network.name.as_str())
                .collect()
        }
        fn cast(&self) -> Vec<&Self::Cast> {
            self.cast.iter().collect()
        }
        fn crew(&self) -> Vec<&Self::Crew> {
            self.crew.iter().collect()
        }
        fn keywords(&self) -> Vec<&str> {
            self.keywords
                .iter()
                .map(|keyword| keyword.name.as_str())
                .collect()
        }
        fn external_ids(&self) -> &Self::ExIds {
            &self.external_ids
        }
    }

    impl SeasonDetailsLike for ArchivedSeasonDetails {
        fn num_episodes(&self) -> u16 {
            self.episode_count.to_native()
        }
    }
}
