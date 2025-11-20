use rkyv::{
    Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize, option::ArchivedOption,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::{
    ArchivedCastMember, ArchivedCrewMember, ArchivedEnhancedSeriesDetails, ArchivedExternalIds,
    ArchivedSeasonDetails, CastMember, CrewMember, EnhancedMovieDetails, EnhancedSeriesDetails,
    EpisodeDetails, ExternalIds, SeasonDetails, TmdbDetails, types::LibraryID,
};

pub trait MediaDetails {
    type MovieDetails;
    type SeriesDetails;
    type SeasonDetails;
    type EpisodeDetails;

    fn to_movie_details(&self) -> Option<&Self::MovieDetails>;
    fn to_series_details(&self) -> Option<&Self::SeriesDetails>;
    fn to_season_details(&self) -> Option<&Self::SeasonDetails>;
    fn to_episode_details(&self) -> Option<&Self::EpisodeDetails>;
}

impl MediaDetails for TmdbDetails {
    type MovieDetails = EnhancedMovieDetails;
    type SeriesDetails = EnhancedSeriesDetails;
    type SeasonDetails = SeasonDetails;
    type EpisodeDetails = EpisodeDetails;

    fn to_movie_details(&self) -> Option<&EnhancedMovieDetails> {
        match self {
            TmdbDetails::Movie(details) => Some(details),
            _ => None,
        }
    }

    fn to_series_details(&self) -> Option<&EnhancedSeriesDetails> {
        match self {
            TmdbDetails::Series(details) => Some(details),
            _ => None,
        }
    }

    fn to_season_details(&self) -> Option<&SeasonDetails> {
        match self {
            TmdbDetails::Season(details) => Some(details),
            _ => None,
        }
    }

    fn to_episode_details(&self) -> Option<&EpisodeDetails> {
        match self {
            TmdbDetails::Episode(details) => Some(details),
            _ => None,
        }
    }
}

pub trait SeriesDetailsLike {
    type Cast;
    type Crew;
    type ExIds;

    fn tmdb_id(&self) -> u64;
    fn name(&self) -> &str;
    fn overview(&self) -> Option<&str>;
    fn first_air_date(&self) -> Option<&str>;
    fn last_air_date(&self) -> Option<&str>;
    fn num_seasons(&self) -> Option<u32>;
    fn num_episodes(&self) -> Option<u32>;
    fn vote_average(&self) -> Option<f32>;
    fn vote_count(&self) -> Option<u32>;
    fn popularity(&self) -> Option<f32>;
    fn genres(&self) -> Vec<&str>;
    fn networks(&self) -> Vec<&str>;
    fn cast(&self) -> Vec<&Self::Cast>;
    fn crew(&self) -> Vec<&Self::Crew>;
    fn keywords(&self) -> Vec<&str>;
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
    fn num_seasons(&self) -> Option<u32> {
        self.number_of_seasons
    }
    fn num_episodes(&self) -> Option<u32> {
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
        self.genres.iter().map(|genre| genre.as_str()).collect()
    }
    fn networks(&self) -> Vec<&str> {
        self.networks
            .iter()
            .map(|network| network.as_str())
            .collect()
    }
    fn cast(&self) -> Vec<&Self::Cast> {
        self.cast.iter().map(|cast| cast).collect()
    }
    fn crew(&self) -> Vec<&Self::Crew> {
        self.crew.iter().map(|crew| crew).collect()
    }
    fn keywords(&self) -> Vec<&str> {
        self.keywords
            .iter()
            .map(|keyword| keyword.as_str())
            .collect()
    }
    fn external_ids(&self) -> &Self::ExIds {
        &self.external_ids
    }
}

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
    fn num_seasons(&self) -> Option<u32> {
        if let ArchivedOption::Some(seasons) = self.number_of_seasons {
            Some(seasons.to_native())
        } else {
            None
        }
    }
    fn num_episodes(&self) -> Option<u32> {
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
        self.genres.iter().map(|genre| genre.as_str()).collect()
    }
    fn networks(&self) -> Vec<&str> {
        self.networks
            .iter()
            .map(|network| network.as_str())
            .collect()
    }
    fn cast(&self) -> Vec<&Self::Cast> {
        self.cast.iter().map(|cast| cast).collect()
    }
    fn crew(&self) -> Vec<&Self::Crew> {
        self.crew.iter().map(|crew| crew).collect()
    }
    fn keywords(&self) -> Vec<&str> {
        self.keywords
            .iter()
            .map(|keyword| keyword.as_str())
            .collect()
    }
    fn external_ids(&self) -> &Self::ExIds {
        &self.external_ids
    }
}

pub trait SeasonDetailsLike {
    fn num_episodes(&self) -> u32;
}

impl SeasonDetailsLike for SeasonDetails {
    fn num_episodes(&self) -> u32 {
        self.episode_count
    }
}

impl SeasonDetailsLike for ArchivedSeasonDetails {
    fn num_episodes(&self) -> u32 {
        self.episode_count.to_native()
    }
}
