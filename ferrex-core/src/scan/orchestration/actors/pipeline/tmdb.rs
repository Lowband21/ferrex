use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{Map, Value};
use tracing::{debug, warn};

use crate::database::ports::media_files::MediaFilesWritePort;
use crate::database::ports::media_references::MediaReferencesRepository;
use crate::error::{MediaError, Result};
use crate::image::MediaImageKind;
use crate::image::records::MediaImageVariantKey;
use crate::image_service::{ImageService, TmdbImageSize};
use crate::orchestration::actors::messages::ParentDescriptors;
use crate::orchestration::job::{
    ImageFetchJob, ImageFetchPriority, ImageFetchSource,
};
use crate::orchestration::series::{
    SeriesFolderClues, SeriesLocator, clean_series_title, collapse_whitespace,
};
use crate::providers::{ProviderError, TmdbApiProvider};
use crate::traits::prelude::MediaIDLike;
use crate::tv_parser::TvParser;
use crate::types::details::{
    AlternativeTitle, CastMember, CollectionInfo, ContentRating, CrewMember,
    EnhancedMovieDetails, EnhancedSeriesDetails, EpisodeDetails, ExternalIds,
    GenreInfo, Keyword, MediaDetailsOption, NetworkInfo, PersonExternalIds,
    ProductionCompany, ProductionCountry, RelatedMediaRef, ReleaseDateEntry,
    ReleaseDatesByCountry, SeasonDetails, SpokenLanguage, TmdbDetails,
    Translation, Video,
};
use crate::types::files::{MediaFile, MediaFileMetadata, ParsedMediaInfo};
use crate::types::ids::{EpisodeID, LibraryID, MovieID, SeasonID, SeriesID};
use crate::types::image::MediaImages;
use crate::types::library::LibraryType;
use crate::types::media::{
    EpisodeReference, MovieReference, SeasonReference, SeriesReference,
};
use crate::types::numbers::{EpisodeNumber, SeasonNumber};
use crate::types::titles::{MovieTitle, SeriesTitle};
use crate::types::urls::{EpisodeURL, MovieURL, SeasonURL, SeriesURL, UrlLike};
use tmdb_api::{
    common::release_date::ReleaseDateKind,
    movie::{
        alternative_titles::MovieAlternativeTitlesResult,
        credits::MovieCreditsResult, external_ids::MovieExternalIdsResult,
        keywords::MovieKeywordsResult, release_dates::MovieReleaseDatesResult,
        translations::MovieTranslationsResult, videos::MovieVideosResult,
    },
    tvshow::{
        Season as TmdbSeason, aggregate_credits::TVShowAggregateCreditsResult,
        content_rating::ContentRatingResult as TvContentRatingResult,
    },
};
use uuid::Uuid;

use super::{
    DefaultMetadataActor, MediaReadyForIndex, MetadataActor, MetadataCommand,
};

static MOVIE_FOLDER_YEAR_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(.+?)\s*\((\d{4})\)(?:\s.+)?\s*$")
        .expect("movie folder year regex should compile")
});
static MOVIE_FILENAME_YEAR_PARENS_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(.+?)\s*\((\d{4})\)")
        .expect("movie filename paren regex should compile")
});
static MOVIE_FILENAME_YEAR_DOT_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(.+?)[\.\s]+(\d{4})[\.\s]")
        .expect("movie filename dot regex should compile")
});
const SEASON_NOT_FOUND_PREFIX: &str = "season_not_found";
const EPISODE_NOT_FOUND_PREFIX: &str = "episode_not_found";

pub struct TmdbMetadataActor {
    media_refs: Arc<dyn MediaReferencesRepository>,
    media_files_write: Arc<dyn MediaFilesWritePort>,
    tmdb: Arc<TmdbApiProvider>,
    image_service: Arc<ImageService>,
}

impl fmt::Debug for TmdbMetadataActor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TmdbMetadataActor").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
struct EpisodeContextInfo {
    series: SeriesFolderClues,
    season_number: u32,
    episode_number: u32,
    episode_title: Option<String>,
    episode_year: Option<u16>,
}

impl TmdbMetadataActor {
    pub fn new(
        media_refs: Arc<dyn MediaReferencesRepository>,
        media_files_write: Arc<dyn MediaFilesWritePort>,
        tmdb: Arc<TmdbApiProvider>,
        image_service: Arc<ImageService>,
    ) -> Self {
        Self {
            media_refs,
            media_files_write,
            tmdb,
            image_service,
        }
    }

    fn ensure_context_object(context: &mut Value) -> &mut Map<String, Value> {
        if !context.is_object() {
            *context = Value::Object(Map::new());
        }
        context
            .as_object_mut()
            .expect("context should be JSON object after initialization")
    }

    fn infer_media_kind(context: &Value) -> MetadataMediaKind {
        context
            .as_object()
            .and_then(|obj| obj.get("media_kind"))
            .and_then(|val| val.as_str())
            .map(|raw| match raw {
                "Movie" => MetadataMediaKind::Movie,
                "Episode" => MetadataMediaKind::Episode,
                _ => MetadataMediaKind::Unknown,
            })
            .unwrap_or(MetadataMediaKind::Unknown)
    }

    fn extract_technical_metadata(
        context: &Value,
    ) -> Option<MediaFileMetadata> {
        context
            .as_object()
            .and_then(|obj| obj.get("technical_metadata"))
            .and_then(|value| serde_json::from_value(value.clone()).ok())
    }

    fn derive_movie_info(
        metadata: Option<&MediaFileMetadata>,
        path: &Path,
    ) -> (String, Option<u16>) {
        if let Some(meta) = metadata
            && let Some(ParsedMediaInfo::Movie(info)) = &meta.parsed_info
        {
            return (info.title.clone(), info.year);
        }

        if let Some(folder_name) = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
        {
            let (title, year) = Self::parse_movie_folder_name(folder_name);
            if !title.is_empty() {
                return (title, year);
            }
        }

        let filename = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        Self::parse_movie_filename(filename)
    }

    fn parse_movie_folder_name(name: &str) -> (String, Option<u16>) {
        if let Some(captures) = MOVIE_FOLDER_YEAR_PATTERN.captures(name) {
            let title =
                captures.get(1).map(|m| m.as_str().trim()).unwrap_or("");
            let year =
                captures.get(2).and_then(|m| m.as_str().parse::<u16>().ok());
            return (title.to_string(), year);
        }
        (name.trim().to_string(), None)
    }

    fn parse_movie_filename(name: &str) -> (String, Option<u16>) {
        if let Some(captures) =
            MOVIE_FILENAME_YEAR_PARENS_PATTERN.captures(name)
        {
            let title = captures
                .get(1)
                .map(|m| m.as_str().replace(['.', '_'], " ").trim().to_string())
                .unwrap_or_else(String::new);
            let year =
                captures.get(2).and_then(|m| m.as_str().parse::<u16>().ok());
            return (title, year);
        }

        if let Some(captures) = MOVIE_FILENAME_YEAR_DOT_PATTERN.captures(name) {
            let title = captures
                .get(1)
                .map(|m| m.as_str().replace(['.', '_'], " ").trim().to_string())
                .unwrap_or_else(String::new);
            let year =
                captures.get(2).and_then(|m| m.as_str().parse::<u16>().ok());
            return (title, year);
        }

        let cleaned = name
            .split(['[', '(', '{'])
            .next()
            .unwrap_or(name)
            .replace(['.', '_', '-'], " ")
            .trim()
            .to_string();
        (cleaned, None)
    }

    fn derive_episode_info(
        metadata: Option<&MediaFileMetadata>,
        path: &Path,
    ) -> Option<EpisodeContextInfo> {
        let folder_clues = SeriesFolderClues::from_path(path);

        if let Some(meta) = metadata
            && let Some(ParsedMediaInfo::Episode(info)) = &meta.parsed_info
        {
            let clues = folder_clues
                .clone()
                .merge_metadata(Some(info.show_name.as_str()), info.year);

            return Some(EpisodeContextInfo {
                series: clues,
                season_number: info.season,
                episode_number: info.episode,
                episode_title: info.episode_title.clone(),
                episode_year: info.year,
            });
        }

        TvParser::parse_episode_info(path).map(|info| {
            let clues = folder_clues;
            let episode_title = TvParser::extract_episode_title(path);
            let episode_year =
                info.year.and_then(|value| u16::try_from(value).ok());

            EpisodeContextInfo {
                series: clues,
                season_number: info.season,
                episode_number: info.episode,
                episode_title,
                episode_year,
            }
        })
    }

    fn parse_parent_descriptors(context: &Value) -> Option<ParentDescriptors> {
        context
            .as_object()
            .and_then(|obj| obj.get("parent"))
            .cloned()
            .and_then(|raw| {
                serde_json::from_value::<ParentDescriptors>(raw).ok()
            })
    }

    async fn queue_image_job(
        &self,
        library_id: LibraryID,
        media_type: &str,
        media_id: Uuid,
        image_type: MediaImageKind,
        order_index: i32,
        tmdb_path: Option<&str>,
        is_primary: bool,
        priority_hint: ImageFetchPriority,
        jobs: &mut Vec<ImageFetchJob>,
    ) -> Result<()> {
        let Some(tmdb_path) =
            tmdb_path.map(str::trim).filter(|path| !path.is_empty())
        else {
            return Ok(());
        };

        self.image_service
            .link_to_media(
                media_type,
                media_id,
                tmdb_path,
                image_type.clone(),
                order_index,
                is_primary,
            )
            .await?;

        for size in TmdbImageSize::recommended_for_kind(&image_type) {
            jobs.push(ImageFetchJob {
                library_id,
                source: ImageFetchSource::Tmdb {
                    tmdb_path: tmdb_path.to_string(),
                },
                key: MediaImageVariantKey {
                    media_type: media_type.to_string(),
                    media_id,
                    image_type: image_type.clone(),
                    order_index,
                    variant: size.as_str().to_string(),
                },
                priority_hint,
            });
        }

        Ok(())
    }

    async fn queue_local_episode_thumbnail(
        &self,
        library_id: LibraryID,
        episode: &EpisodeReference,
        jobs: &mut Vec<ImageFetchJob>,
    ) -> Result<()> {
        let image_key =
            format!("local/episode/{}/thumbnail.jpg", episode.file.id);

        self.image_service
            .link_to_media(
                "episode",
                episode.id.0,
                &image_key,
                MediaImageKind::Thumbnail,
                0,
                true,
            )
            .await?;

        jobs.push(ImageFetchJob {
            library_id,
            source: ImageFetchSource::EpisodeThumbnail {
                media_file_id: episode.file.id,
                image_key,
            },
            key: MediaImageVariantKey {
                media_type: "episode".to_string(),
                media_id: episode.id.0,
                image_type: MediaImageKind::Thumbnail,
                order_index: 0,
                variant: "original".to_string(),
            },
            priority_hint: ImageFetchPriority::Backdrop,
        });

        Ok(())
    }

    fn person_media_uuid(tmdb_person_id: u64) -> Uuid {
        Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            format!("person-{}", tmdb_person_id).as_bytes(),
        )
    }

    async fn queue_person_profile_jobs(
        &self,
        library_id: LibraryID,
        cast: &[CastMember],
        jobs: &mut Vec<ImageFetchJob>,
    ) -> Result<()> {
        let mut seen_people = HashSet::new();

        for member in cast {
            if !seen_people.insert(member.id) {
                continue;
            }

            let Some(path) = member
                .profile_path
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
            else {
                continue;
            };

            if member.image_slot == u32::MAX {
                continue;
            }

            let person_uuid = Self::person_media_uuid(member.id);

            self.queue_image_job(
                library_id,
                "person",
                person_uuid,
                MediaImageKind::Cast,
                member.image_slot as i32,
                Some(path),
                member.image_slot == 0,
                ImageFetchPriority::Profile,
                jobs,
            )
            .await?;
        }

        Ok(())
    }

    fn slugify_title(title: &str) -> String {
        let mut slug = String::new();
        for ch in title.trim().chars() {
            if ch.is_ascii_alphanumeric() {
                slug.push(ch.to_ascii_lowercase());
            } else if (ch.is_whitespace()
                || matches!(ch, '.' | '_' | '-' | '/' | '\\'))
                && !slug.ends_with('-')
            {
                slug.push('-');
            }
        }
        slug.trim_matches('-').to_string()
    }

    fn handle_movie_release_dates(
        tmdb_id: u64,
        result: std::result::Result<
            MovieReleaseDatesResult,
            crate::providers::ProviderError,
        >,
    ) -> Option<(
        Option<String>,
        Vec<ContentRating>,
        Vec<ReleaseDatesByCountry>,
    )> {
        match result {
            Ok(data) => {
                let certification = Self::extract_movie_certification(&data);
                let release_dates = Self::map_movie_release_dates(&data);
                let content_ratings = Self::map_movie_content_ratings(&data);
                Some((certification, content_ratings, release_dates))
            }
            Err(err) => {
                warn!(
                    "Failed to fetch movie release dates for {}: {}",
                    tmdb_id, err
                );
                None
            }
        }
    }

    fn release_date_kind_to_i32(kind: &ReleaseDateKind) -> i32 {
        match kind {
            ReleaseDateKind::Premiere => ReleaseDateKind::Premiere as i32,
            ReleaseDateKind::TheatricalLimited => {
                ReleaseDateKind::TheatricalLimited as i32
            }
            ReleaseDateKind::Theatrical => ReleaseDateKind::Theatrical as i32,
            ReleaseDateKind::Digital => ReleaseDateKind::Digital as i32,
            ReleaseDateKind::Physical => ReleaseDateKind::Physical as i32,
            ReleaseDateKind::TV => ReleaseDateKind::TV as i32,
        }
    }

    fn extract_movie_certification(
        data: &MovieReleaseDatesResult,
    ) -> Option<String> {
        let preferred = [
            ReleaseDateKind::Theatrical,
            ReleaseDateKind::TheatricalLimited,
            ReleaseDateKind::Digital,
            ReleaseDateKind::Physical,
            ReleaseDateKind::TV,
            ReleaseDateKind::Premiere,
        ];

        let pick_cert =
            |dates: &[tmdb_api::common::release_date::ReleaseDate]| {
                for kind in preferred.iter() {
                    if let Some(cert) = dates
                        .iter()
                        .filter(|rd| &rd.kind == kind)
                        .filter_map(|rd| rd.certification.as_ref())
                        .find(|cert| !cert.trim().is_empty())
                    {
                        return Some(cert.trim().to_string());
                    }
                }
                dates
                    .iter()
                    .filter_map(|rd| rd.certification.as_ref())
                    .find(|cert| !cert.trim().is_empty())
                    .map(|cert| cert.trim().to_string())
            };

        for region in ["US", "GB", "CA", "AU", "NZ", "FR"] {
            if let Some(entry) =
                data.results.iter().find(|r| r.iso_3166_1 == region)
                && let Some(cert) = pick_cert(&entry.release_dates)
            {
                return Some(cert);
            }
        }

        data.results
            .iter()
            .filter_map(|entry| pick_cert(&entry.release_dates))
            .next()
    }

    fn map_movie_release_dates(
        data: &MovieReleaseDatesResult,
    ) -> Vec<ReleaseDatesByCountry> {
        data.results
            .iter()
            .map(|entry| ReleaseDatesByCountry {
                iso_3166_1: entry.iso_3166_1.clone(),
                release_dates: entry
                    .release_dates
                    .iter()
                    .map(|rd| ReleaseDateEntry {
                        certification: rd.certification.clone(),
                        release_date: Some(rd.release_date.to_rfc3339()),
                        release_type: Some(Self::release_date_kind_to_i32(
                            &rd.kind,
                        )),
                        note: rd.note.clone(),
                        iso_639_1: rd.iso_639_1.clone(),
                        descriptors: Vec::new(),
                    })
                    .collect(),
            })
            .collect()
    }

    fn map_movie_content_ratings(
        data: &MovieReleaseDatesResult,
    ) -> Vec<ContentRating> {
        data.results
            .iter()
            .filter_map(|entry| {
                entry
                    .release_dates
                    .iter()
                    .find_map(|rd| rd.certification.as_ref())
                    .map(|cert| ContentRating {
                        iso_3166_1: entry.iso_3166_1.clone(),
                        rating: Some(cert.clone()),
                        rating_system: None,
                        descriptors: Vec::new(),
                    })
            })
            .collect()
    }

    fn map_movie_keywords(result: &MovieKeywordsResult) -> Vec<Keyword> {
        result
            .keywords
            .iter()
            .map(|keyword| Keyword {
                id: keyword.id,
                name: keyword.name.clone(),
            })
            .collect()
    }

    fn map_movie_videos(result: &MovieVideosResult) -> Vec<Video> {
        result
            .results
            .iter()
            .map(|video| Video {
                key: video.key.clone(),
                name: Some(video.name.clone()),
                site: video.site.clone(),
                video_type: Some(video.kind.clone()),
                official: None,
                iso_639_1: Some(video.iso_639_1.clone()),
                iso_3166_1: Some(video.iso_3166_1.clone()),
                published_at: Some(video.published_at.to_rfc3339()),
                size: u32::try_from(video.size).ok(),
            })
            .collect()
    }

    fn map_movie_translations(
        result: &MovieTranslationsResult,
    ) -> Vec<Translation> {
        result
            .translations
            .iter()
            .map(|translation| Translation {
                iso_3166_1: translation.iso_3166_1.clone(),
                iso_639_1: translation.iso_639_1.clone(),
                name: Some(translation.name.clone()),
                english_name: Some(translation.english_name.clone()),
                title: translation.data.title.clone(),
                overview: translation.data.overview.clone(),
                homepage: translation.data.homepage.clone(),
                tagline: None,
            })
            .collect()
    }

    fn map_movie_alternative_titles(
        result: &MovieAlternativeTitlesResult,
    ) -> Vec<AlternativeTitle> {
        let mut seen = HashSet::new();

        result
            .titles
            .iter()
            .filter_map(|title| {
                let iso_trimmed = title.iso_3166_1.trim();
                let iso_3166_1 = if iso_trimmed.is_empty() {
                    None
                } else {
                    Some(iso_trimmed.to_string())
                };

                let title_type =
                    title.kind.as_ref().map(|kind| kind.trim()).and_then(
                        |kind| {
                            if kind.is_empty() {
                                None
                            } else {
                                Some(kind.to_string())
                            }
                        },
                    );

                let title_text = title.title.trim();
                if title_text.is_empty() {
                    return None;
                }

                let key = (
                    iso_3166_1.clone().unwrap_or_default(),
                    title_type.clone().unwrap_or_default(),
                    title_text.to_string(),
                );

                if !seen.insert(key) {
                    return None;
                }

                Some(AlternativeTitle {
                    title: title_text.to_string(),
                    iso_3166_1,
                    title_type,
                })
            })
            .collect()
    }

    fn map_related_movies(
        result: &tmdb_api::common::PaginatedResult<tmdb_api::movie::MovieShort>,
    ) -> Vec<RelatedMediaRef> {
        result
            .results
            .iter()
            .map(|movie| RelatedMediaRef {
                tmdb_id: movie.inner.id,
                title: Some(movie.inner.title.clone()),
            })
            .collect()
    }

    fn map_movie_external_ids(result: &MovieExternalIdsResult) -> ExternalIds {
        ExternalIds {
            imdb_id: result.imdb_id.clone(),
            tvdb_id: None,
            facebook_id: result.facebook_id.clone(),
            instagram_id: result.instagram_id.clone(),
            twitter_id: result.twitter_id.clone(),
            wikidata_id: None,
            tiktok_id: None,
            youtube_id: None,
            freebase_id: None,
            freebase_mid: None,
        }
    }

    fn map_cast(credits: &MovieCreditsResult) -> Vec<CastMember> {
        let mut next_slot: u32 = 0;
        credits
            .cast
            .iter()
            .take(20)
            .map(|c| {
                let person_uuid = Self::person_media_uuid(c.person.id);
                let slot = if c.person.profile_path.is_some() {
                    let assigned = next_slot;
                    next_slot = next_slot.saturating_add(1);
                    assigned
                } else {
                    u32::MAX
                };

                CastMember {
                    id: c.person.id,
                    credit_id: Some(c.credit.credit_id.clone()),
                    cast_id: Some(c.cast_id),
                    name: c.person.name.clone(),
                    original_name: Some(c.credit.original_name.clone()),
                    character: c.character.clone(),
                    profile_path: c.person.profile_path.clone(),
                    order: c.order as u32,
                    gender: c.person.gender.map(|g| g as u8),
                    known_for_department: c.credit.known_for_department.clone(),
                    adult: Some(c.credit.adult),
                    popularity: Some(c.credit.popularity as f32),
                    also_known_as: Vec::new(),
                    external_ids: PersonExternalIds::default(),
                    image_slot: slot,
                    profile_media_id: c
                        .person
                        .profile_path
                        .as_ref()
                        .map(|_| person_uuid),
                    profile_image_index: c
                        .person
                        .profile_path
                        .as_ref()
                        .map(|_| slot),
                }
            })
            .collect()
    }

    fn map_crew(credits: &MovieCreditsResult) -> Vec<CrewMember> {
        credits
            .crew
            .iter()
            .filter(|c| {
                matches!(
                    c.job.as_str(),
                    "Director"
                        | "Producer"
                        | "Writer"
                        | "Director of Photography"
                )
            })
            .take(10)
            .map(|c| CrewMember {
                id: c.person.id,
                credit_id: Some(c.credit.credit_id.clone()),
                name: c.person.name.clone(),
                job: c.job.clone(),
                department: c.department.clone(),
                profile_path: c.person.profile_path.clone(),
                gender: c.person.gender.map(|g| g as u8),
                known_for_department: c.credit.known_for_department.clone(),
                adult: Some(c.credit.adult),
                popularity: Some(c.credit.popularity as f32),
                original_name: Some(c.credit.original_name.clone()),
                also_known_as: Vec::new(),
                external_ids: PersonExternalIds::default(),
            })
            .collect()
    }

    async fn fetch_series_content_rating(
        &self,
        tmdb_id: u64,
    ) -> (Option<String>, Vec<ContentRating>) {
        match self.tmdb.get_tv_content_ratings(tmdb_id).await {
            Ok(result) => {
                let primary = Self::extract_series_content_rating(&result);
                let ratings = Self::map_series_content_ratings(&result);
                (primary, ratings)
            }
            Err(err) => {
                warn!(
                    "Failed to fetch TV content ratings for {}: {}",
                    tmdb_id, err
                );
                (None, Vec::new())
            }
        }
    }

    fn extract_series_content_rating(
        data: &TvContentRatingResult,
    ) -> Option<String> {
        let preferred_regions = ["US", "GB", "CA", "AU", "NZ", "FR"];
        for region in preferred_regions {
            if let Some(entry) =
                data.results.iter().find(|r| r.iso_3166_1 == region)
                && !entry.rating.trim().is_empty()
            {
                return Some(entry.rating.trim().to_string());
            }
        }

        data.results
            .iter()
            .find(|entry| !entry.rating.trim().is_empty())
            .map(|entry| entry.rating.trim().to_string())
    }

    fn map_series_content_ratings(
        data: &TvContentRatingResult,
    ) -> Vec<ContentRating> {
        let mut ratings = Vec::new();
        let mut index_by_iso = HashMap::new();

        for entry in &data.results {
            let iso = match Self::normalize_region_code(&entry.iso_3166_1) {
                Some(code) => code,
                None => continue,
            };

            let mut candidate = ContentRating {
                iso_3166_1: iso.clone(),
                rating: Self::normalize_rating_value(&entry.rating),
                rating_system: None,
                descriptors: Self::normalize_descriptors(&entry.descriptors),
            };

            if let Some(&idx) = index_by_iso.get(&iso) {
                let existing = ratings
                    .get_mut(idx)
                    .expect("series rating index should be valid");
                if Self::should_replace_rating(existing, &candidate) {
                    Self::merge_descriptors(
                        &mut candidate,
                        &existing.descriptors,
                    );
                    *existing = candidate;
                } else {
                    Self::merge_descriptors(existing, &candidate.descriptors);
                }
            } else {
                index_by_iso.insert(iso, ratings.len());
                ratings.push(candidate);
            }
        }

        ratings
    }

    fn normalize_region_code(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_uppercase())
        }
    }

    fn normalize_rating_value(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }

        let normalized_value = collapse_whitespace(trimmed);
        let normalized = normalized_value.trim();
        if normalized.is_empty() {
            None
        } else {
            Some(normalized.to_string())
        }
    }

    fn normalize_descriptors(values: &[String]) -> Vec<String> {
        let mut normalized = Vec::new();
        let mut seen = HashSet::new();
        for value in values {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                continue;
            }

            let collapsed = collapse_whitespace(trimmed);
            let candidate = collapsed.trim().to_string();
            if candidate.is_empty() {
                continue;
            }

            if seen.insert(candidate.clone()) {
                normalized.push(candidate);
            }
        }

        normalized
    }

    fn should_replace_rating(
        existing: &ContentRating,
        candidate: &ContentRating,
    ) -> bool {
        match (&existing.rating, &candidate.rating) {
            (None, Some(_)) => true,
            (Some(_), None) => false,
            (None, None) => {
                candidate.descriptors.len() > existing.descriptors.len()
            }
            (Some(existing_value), Some(candidate_value)) => {
                let existing_key = Self::rating_compare_key(existing_value);
                let candidate_key = Self::rating_compare_key(candidate_value);
                if existing_key != candidate_key {
                    return false;
                }

                if candidate_value.len() < existing_value.len() {
                    return true;
                }

                candidate.descriptors.len() > existing.descriptors.len()
            }
        }
    }

    fn merge_descriptors(target: &mut ContentRating, additional: &[String]) {
        if additional.is_empty() {
            return;
        }

        let mut seen = target
            .descriptors
            .iter()
            .cloned()
            .collect::<HashSet<String>>();
        for descriptor in additional {
            if seen.insert(descriptor.clone()) {
                target.descriptors.push(descriptor.clone());
            }
        }
    }

    fn rating_compare_key(value: &str) -> String {
        value
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_uppercase()
    }

    fn map_series_cast(
        credits: &TVShowAggregateCreditsResult,
    ) -> Vec<CastMember> {
        let mut next_slot: u32 = 0;
        credits
            .cast
            .iter()
            .take(20)
            .map(|c| {
                let person_uuid = Self::person_media_uuid(c.inner.id);
                let slot = if c.inner.profile_path.is_some() {
                    let assigned = next_slot;
                    next_slot = next_slot.saturating_add(1);
                    assigned
                } else {
                    u32::MAX
                };

                CastMember {
                    id: c.inner.id,
                    credit_id: c
                        .roles
                        .first()
                        .map(|role| role.credit_id.clone()),
                    cast_id: None,
                    name: c.inner.name.clone(),
                    original_name: Some(c.inner.original_name.clone()),
                    character: c
                        .roles
                        .iter()
                        .map(|role| role.character.clone())
                        .find(|character| !character.is_empty())
                        .unwrap_or_else(|| {
                            c.roles
                                .first()
                                .map(|role| role.character.clone())
                                .unwrap_or_default()
                        }),
                    profile_path: c.inner.profile_path.clone(),
                    order: c.order as u32,
                    gender: match c.inner.gender {
                        0 => None,
                        value => Some(value as u8),
                    },
                    known_for_department: Some(
                        c.inner.known_for_department.clone(),
                    ),
                    adult: Some(c.inner.adult),
                    popularity: Some(c.inner.popularity as f32),
                    also_known_as: Vec::new(),
                    external_ids: PersonExternalIds::default(),
                    image_slot: slot,
                    profile_media_id: c
                        .inner
                        .profile_path
                        .as_ref()
                        .map(|_| person_uuid),
                    profile_image_index: c
                        .inner
                        .profile_path
                        .as_ref()
                        .map(|_| slot),
                }
            })
            .collect()
    }

    fn map_series_crew(
        credits: &TVShowAggregateCreditsResult,
    ) -> Vec<CrewMember> {
        credits
            .crew
            .iter()
            .take(20)
            .map(|c| CrewMember {
                id: c.inner.id,
                credit_id: c.jobs.first().map(|job| job.credit_id.clone()),
                name: c.inner.name.clone(),
                job: c
                    .jobs
                    .iter()
                    .map(|job| job.job.clone())
                    .find(|job| !job.is_empty())
                    .unwrap_or_else(|| c.department.clone()),
                department: c.department.clone(),
                profile_path: c.inner.profile_path.clone(),
                gender: match c.inner.gender {
                    0 => None,
                    value => Some(value as u8),
                },
                known_for_department: Some(
                    c.inner.known_for_department.clone(),
                ),
                adult: Some(c.inner.adult),
                popularity: Some(c.inner.popularity as f32),
                original_name: Some(c.inner.original_name.clone()),
                also_known_as: Vec::new(),
                external_ids: PersonExternalIds::default(),
            })
            .collect()
    }

    fn annotate_context(context: &mut Value, tmdb_id: u64) {
        match context {
            Value::Object(map) => {
                map.insert("tmdb_id".into(), Value::from(tmdb_id));
            }
            _ => {
                *context = serde_json::json!({ "tmdb_id": tmdb_id });
            }
        }
    }

    async fn enrich_movie(
        &self,
        mut command: MetadataCommand,
    ) -> Result<MediaReadyForIndex> {
        let metadata =
            Self::extract_technical_metadata(&command.analyzed.context);
        let path = PathBuf::from(&command.analyzed.path_norm);
        let (title_hint, year_hint) =
            Self::derive_movie_info(metadata.as_ref(), &path);
        let clean_title = clean_series_title(&title_hint);

        let search_results = self
            .tmdb
            .search_movies(&clean_title, year_hint)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("TMDB search failed: {e}"))
            })?;

        if let Some(candidate) = search_results.first() {
            let tmdb_id = candidate.tmdb_id;
            let movie_ref = self
                .build_movie_reference(
                    command.analyzed.library_id,
                    &command.analyzed.path_norm,
                    metadata.as_ref(),
                    tmdb_id,
                )
                .await?;

            self.media_refs.store_movie_reference(&movie_ref).await?;

            let mut image_jobs = Vec::new();
            if let MediaDetailsOption::Details(TmdbDetails::Movie(details)) =
                &movie_ref.details
            {
                self.queue_image_job(
                    command.job.library_id,
                    "movie",
                    movie_ref.id.0,
                    MediaImageKind::Poster,
                    0,
                    details.poster_path.as_deref(),
                    true,
                    ImageFetchPriority::Poster,
                    &mut image_jobs,
                )
                .await?;
                self.queue_image_job(
                    command.job.library_id,
                    "movie",
                    movie_ref.id.0,
                    MediaImageKind::Backdrop,
                    0,
                    details.backdrop_path.as_deref(),
                    true,
                    ImageFetchPriority::Backdrop,
                    &mut image_jobs,
                )
                .await?;

                self.queue_person_profile_jobs(
                    command.job.library_id,
                    &details.cast,
                    &mut image_jobs,
                )
                .await?;
            }

            Self::annotate_context(&mut command.analyzed.context, tmdb_id);

            Ok(MediaReadyForIndex {
                library_id: command.job.library_id,
                logical_id: Some(movie_ref.id.to_string()),
                normalized_title: Some(movie_ref.title.to_string()),
                analyzed: command.analyzed,
                prepared_at: Utc::now(),
                image_jobs,
            })
        } else {
            self.store_movie_without_tmdb(command, metadata).await
        }
    }

    async fn build_movie_reference(
        &self,
        library_id: LibraryID,
        path_norm: &str,
        metadata: Option<&MediaFileMetadata>,
        tmdb_id: u64,
    ) -> Result<MovieReference> {
        let tmdb_details = self.tmdb.get_movie(tmdb_id).await.map_err(|e| {
            MediaError::Internal(format!("Failed to fetch movie details: {e}"))
        })?;

        let (
            release_dates_res,
            keywords_res,
            videos_res,
            translations_res,
            alternative_titles_res,
            recommendations_res,
            similar_res,
            external_ids_res,
            credits_res,
        ) = tokio::join!(
            self.tmdb.get_movie_release_dates(tmdb_id),
            self.tmdb.get_movie_keywords(tmdb_id),
            self.tmdb.get_movie_videos(tmdb_id),
            self.tmdb.get_movie_translations(tmdb_id),
            self.tmdb.get_movie_alternative_titles(tmdb_id),
            self.tmdb.get_movie_recommendations(tmdb_id),
            self.tmdb.get_movie_similar(tmdb_id),
            self.tmdb.get_movie_external_ids(tmdb_id),
            self.tmdb.get_movie_credits(tmdb_id),
        );

        let (certification, content_ratings, release_dates_list) =
            Self::handle_movie_release_dates(tmdb_id, release_dates_res)
                .unwrap_or_default();

        let keywords = keywords_res
            .map(|res| Self::map_movie_keywords(&res))
            .unwrap_or_else(|err| {
                warn!(
                    "Failed to fetch movie keywords for {}: {}",
                    tmdb_id, err
                );
                Vec::new()
            });

        let videos = videos_res
            .map(|res| Self::map_movie_videos(&res))
            .unwrap_or_else(|err| {
                warn!("Failed to fetch movie videos for {}: {}", tmdb_id, err);
                Vec::new()
            });

        let translations = translations_res
            .map(|res| Self::map_movie_translations(&res))
            .unwrap_or_else(|err| {
                warn!(
                    "Failed to fetch movie translations for {}: {}",
                    tmdb_id, err
                );
                Vec::new()
            });

        let alternative_titles = alternative_titles_res
            .map(|res| Self::map_movie_alternative_titles(&res))
            .unwrap_or_else(|err| {
                warn!(
                    "Failed to fetch movie alternative titles for {}: {}",
                    tmdb_id, err
                );
                Vec::new()
            });

        let recommendations = recommendations_res
            .map(|res| Self::map_related_movies(&res))
            .unwrap_or_else(|err| {
                warn!(
                    "Failed to fetch movie recommendations for {}: {}",
                    tmdb_id, err
                );
                Vec::new()
            });

        let similar = similar_res
            .map(|res| Self::map_related_movies(&res))
            .unwrap_or_else(|err| {
                warn!(
                    "Failed to fetch similar movies for {}: {}",
                    tmdb_id, err
                );
                Vec::new()
            });

        let external_ids = external_ids_res
            .map(|res| Self::map_movie_external_ids(&res))
            .unwrap_or_else(|err| {
                warn!(
                    "Failed to fetch movie external ids for {}: {}",
                    tmdb_id, err
                );
                ExternalIds::default()
            });

        let credits = credits_res.ok();

        let genres = tmdb_details
            .genres
            .iter()
            .map(|g| GenreInfo {
                id: g.id,
                name: g.name.clone(),
            })
            .collect::<Vec<_>>();
        let production_companies = tmdb_details
            .production_companies
            .iter()
            .map(|c| ProductionCompany {
                id: c.id,
                name: c.name.clone(),
                origin_country: c.origin_country.clone(),
            })
            .collect::<Vec<_>>();
        let production_countries = tmdb_details
            .production_countries
            .iter()
            .map(|country| ProductionCountry {
                iso_3166_1: country.iso_3166_1.clone(),
                name: country.name.clone(),
            })
            .collect::<Vec<_>>();
        let spoken_languages = tmdb_details
            .spoken_languages
            .iter()
            .map(|lang| SpokenLanguage {
                iso_639_1: Some(lang.iso_639_1.clone()),
                name: lang.name.clone(),
            })
            .collect::<Vec<_>>();

        let mut media_file =
            MediaFile::new(PathBuf::from(path_norm), library_id)?;
        if let Some(meta) = metadata {
            media_file.media_file_metadata = Some(meta.clone());
        }

        let upsert = self.media_files_write.upsert(media_file.clone()).await?;
        let actual_file_id = upsert.id;
        media_file.id = actual_file_id;

        let movie_id = MovieID::new();
        let cast = credits.as_ref().map(Self::map_cast).unwrap_or_default();
        let crew = credits.as_ref().map(Self::map_crew).unwrap_or_default();

        let collection =
            tmdb_details
                .belongs_to_collection
                .as_ref()
                .map(|collection| CollectionInfo {
                    id: collection.id,
                    name: collection.name.clone(),
                    poster_path: collection.poster_path.clone(),
                    backdrop_path: collection.backdrop_path.clone(),
                });

        let enhanced = EnhancedMovieDetails {
            id: tmdb_details.inner.id as u64,
            title: tmdb_details.inner.title.clone(),
            original_title: Some(tmdb_details.inner.original_title.clone()),
            overview: Some(tmdb_details.inner.overview.clone()),
            release_date: tmdb_details
                .inner
                .release_date
                .as_ref()
                .map(|d| d.to_string()),
            runtime: tmdb_details.runtime.map(|r| r as u32),
            vote_average: Some(tmdb_details.inner.vote_average as f32),
            vote_count: Some(tmdb_details.inner.vote_count as u32),
            popularity: Some(tmdb_details.inner.popularity as f32),
            content_rating: certification,
            content_ratings,
            release_dates: release_dates_list,
            genres,
            spoken_languages,
            production_companies,
            production_countries,
            poster_path: tmdb_details.inner.poster_path.clone(),
            backdrop_path: tmdb_details.inner.backdrop_path.clone(),
            logo_path: None,
            images: MediaImages::default(),
            cast,
            crew,
            videos,
            keywords,
            external_ids,
            alternative_titles,
            translations,
            collection,
            recommendations,
            similar,
            homepage: tmdb_details.homepage.clone(),
            status: Some(format!("{:?}", tmdb_details.status)),
            tagline: tmdb_details.tagline.clone(),
            budget: Some(tmdb_details.budget),
            revenue: Some(tmdb_details.revenue),
        };

        let movie_ref = MovieReference {
            id: movie_id,
            library_id,
            tmdb_id,
            title: MovieTitle::new(tmdb_details.inner.title.clone()).map_err(
                |e| MediaError::Internal(format!("Invalid movie title: {e}")),
            )?,
            details: MediaDetailsOption::Details(TmdbDetails::Movie(enhanced)),
            endpoint: MovieURL::from_string(format!(
                "/stream/{actual_file_id}"
            )),
            file: media_file,
            theme_color: None,
        };

        Ok(movie_ref)
    }

    async fn store_movie_without_tmdb(
        &self,
        mut command: MetadataCommand,
        metadata: Option<MediaFileMetadata>,
    ) -> Result<MediaReadyForIndex> {
        let mut media_file = MediaFile::new(
            PathBuf::from(&command.analyzed.path_norm),
            command.analyzed.library_id,
        )?;
        if let Some(meta) = metadata.clone() {
            media_file.media_file_metadata = Some(meta);
        }

        let title = metadata
            .as_ref()
            .and_then(|meta| match &meta.parsed_info {
                Some(ParsedMediaInfo::Movie(info)) => Some(info.title.clone()),
                _ => None,
            })
            .unwrap_or_else(|| {
                Path::new(&command.analyzed.path_norm)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .replace(['.', '_', '-'], " ")
            });

        let movie_id = MovieID::new();
        let movie_ref = MovieReference {
            id: movie_id,
            library_id: command.analyzed.library_id,
            tmdb_id: 0,
            title: MovieTitle::new(title.clone()).map_err(|e| {
                MediaError::Internal(format!("Invalid movie title: {e}"))
            })?,
            details: MediaDetailsOption::Endpoint(format!(
                "/movie/lookup/{}",
                media_file.id
            )),
            endpoint: MovieURL::from_string(format!(
                "/stream/{}",
                media_file.id
            )),
            file: media_file,
            theme_color: None,
        };

        self.media_refs.store_movie_reference(&movie_ref).await?;

        Self::annotate_context(&mut command.analyzed.context, 0);

        Ok(MediaReadyForIndex {
            library_id: command.job.library_id,
            logical_id: Some(movie_ref.id.to_string()),
            normalized_title: Some(movie_ref.title.to_string()),
            analyzed: command.analyzed,
            prepared_at: Utc::now(),
            image_jobs: Vec::new(),
        })
    }

    async fn enrich_episode(
        &self,
        mut command: MetadataCommand,
    ) -> Result<MediaReadyForIndex> {
        let metadata =
            Self::extract_technical_metadata(&command.analyzed.context);
        let path = PathBuf::from(&command.analyzed.path_norm);

        let Some(info) = Self::derive_episode_info(metadata.as_ref(), &path)
        else {
            return DefaultMetadataActor::new().enrich(command).await;
        };

        let mut image_jobs = Vec::new();

        let parent = Self::parse_parent_descriptors(&command.analyzed.context);

        let mut excluded_series = HashSet::new();
        let (series_ref, season_ref) = loop {
            let candidate_series = self
                .resolve_series(
                    command.job.library_id,
                    &info,
                    parent.as_ref(),
                    &excluded_series,
                )
                .await?;

            match self
                .resolve_season(
                    command.job.library_id,
                    &candidate_series,
                    info.season_number,
                )
                .await
            {
                Ok(season_ref) => break (candidate_series, season_ref),
                Err(MediaError::InvalidMedia(msg))
                    if msg.starts_with(SEASON_NOT_FOUND_PREFIX) =>
                {
                    if candidate_series.tmdb_id == 0 {
                        return Err(MediaError::InvalidMedia(msg));
                    }
                    if !excluded_series.insert(candidate_series.tmdb_id) {
                        return Err(MediaError::InvalidMedia(msg));
                    }
                    continue;
                }
                Err(err) => return Err(err),
            }
        };

        if let MediaDetailsOption::Details(TmdbDetails::Series(details)) =
            &series_ref.details
        {
            self.queue_image_job(
                command.job.library_id,
                "series",
                series_ref.id.0,
                MediaImageKind::Poster,
                0,
                details.poster_path.as_deref(),
                true,
                ImageFetchPriority::Poster,
                &mut image_jobs,
            )
            .await?;
            self.queue_image_job(
                command.job.library_id,
                "series",
                series_ref.id.0,
                MediaImageKind::Backdrop,
                0,
                details.backdrop_path.as_deref(),
                true,
                ImageFetchPriority::Backdrop,
                &mut image_jobs,
            )
            .await?;

            self.queue_person_profile_jobs(
                command.job.library_id,
                &details.cast,
                &mut image_jobs,
            )
            .await?;
        }

        if let MediaDetailsOption::Details(TmdbDetails::Season(details)) =
            &season_ref.details
        {
            self.queue_image_job(
                command.job.library_id,
                "season",
                season_ref.id.0,
                MediaImageKind::Poster,
                0,
                details.poster_path.as_deref(),
                true,
                ImageFetchPriority::Poster,
                &mut image_jobs,
            )
            .await?;
        }

        let (episode_ref, tmdb_episode_id) = self
            .create_episode_reference(
                &command,
                &series_ref,
                &season_ref,
                metadata.clone(),
                &info,
            )
            .await?;

        match &episode_ref.details {
            MediaDetailsOption::Details(TmdbDetails::Episode(details)) => {
                if let Some(still) = details
                    .still_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    self.queue_image_job(
                        command.job.library_id,
                        "episode",
                        episode_ref.id.0,
                        MediaImageKind::Thumbnail,
                        0,
                        Some(still),
                        true,
                        ImageFetchPriority::Backdrop,
                        &mut image_jobs,
                    )
                    .await?;
                } else {
                    self.queue_local_episode_thumbnail(
                        command.job.library_id,
                        &episode_ref,
                        &mut image_jobs,
                    )
                    .await?;
                }
            }
            _ => {
                self.queue_local_episode_thumbnail(
                    command.job.library_id,
                    &episode_ref,
                    &mut image_jobs,
                )
                .await?;
            }
        }

        Self::annotate_episode_context(
            &mut command.analyzed.context,
            &series_ref,
            &season_ref,
            &episode_ref,
            &info,
            tmdb_episode_id,
        );

        let normalized_title = Some(Self::build_episode_normalized_title(
            &series_ref,
            &season_ref,
            &episode_ref,
            &info,
        ));

        Ok(MediaReadyForIndex {
            library_id: command.job.library_id,
            logical_id: Some(episode_ref.id.to_string()),
            normalized_title,
            analyzed: command.analyzed,
            prepared_at: Utc::now(),
            image_jobs,
        })
    }

    async fn resolve_series(
        &self,
        library_id: LibraryID,
        info: &EpisodeContextInfo,
        parent: Option<&ParentDescriptors>,
        excluded_tmdb_ids: &HashSet<u64>,
    ) -> Result<SeriesReference> {
        let locator = SeriesLocator::new(self.media_refs.clone());
        if let Some(existing) = locator
            .find_existing_series(
                library_id,
                parent,
                &info.series.normalized_title,
            )
            .await?
            && (existing.tmdb_id == 0
                || !excluded_tmdb_ids.contains(&existing.tmdb_id))
        {
            return Ok(existing);
        }

        let search_results = self
            .tmdb
            .search_series(
                &info.series.normalized_title,
                info.series.year,
                info.series.region.as_deref(),
            )
            .await
            .map_err(|e| {
                MediaError::Internal(format!("TMDB series search failed: {e}"))
            })?;

        let mut ordered_tmdb_ids = Vec::new();
        let mut seen_ids = HashSet::new();

        let clean_title = info.series.normalized_title.clone();

        if let Some(primary) = Self::pick_series_candidate(
            &clean_title,
            info.series.region.as_deref(),
            &search_results,
        ) && primary.tmdb_id != 0
            && seen_ids.insert(primary.tmdb_id)
        {
            ordered_tmdb_ids.push(primary.tmdb_id);
        }

        for candidate in &search_results {
            let tmdb_id = candidate.tmdb_id;
            if tmdb_id == 0 {
                continue;
            }
            if seen_ids.insert(tmdb_id) {
                ordered_tmdb_ids.push(tmdb_id);
            }
        }

        for tmdb_id in ordered_tmdb_ids {
            if excluded_tmdb_ids.contains(&tmdb_id) {
                continue;
            }

            if let Some(existing) = self
                .media_refs
                .get_series_by_tmdb_id(library_id, tmdb_id)
                .await?
                && (existing.tmdb_id == 0
                    || !excluded_tmdb_ids.contains(&existing.tmdb_id))
            {
                return Ok(existing);
            }

            let mut series_ref =
                self.build_series_reference(library_id, tmdb_id).await?;

            self.media_refs.store_series_reference(&series_ref).await?;

            if let Some(stored) = self
                .media_refs
                .get_series_by_tmdb_id(library_id, tmdb_id)
                .await?
            {
                if stored.id != series_ref.id {
                    series_ref.id = stored.id;
                }
                series_ref.endpoint = stored.endpoint;
                series_ref.created_at = stored.created_at;
                if series_ref.theme_color.is_none() {
                    series_ref.theme_color = stored.theme_color;
                }
            }

            if series_ref.tmdb_id == 0
                || !excluded_tmdb_ids.contains(&series_ref.tmdb_id)
            {
                return Ok(series_ref);
            }
        }

        debug!(
            "Falling back to stub series metadata for '{}'",
            info.series.raw_title
        );
        let stub = self.build_series_stub(library_id, info, parent)?;
        self.media_refs.store_series_reference(&stub).await?;
        Ok(stub)
    }

    async fn build_series_reference(
        &self,
        library_id: LibraryID,
        tmdb_id: u64,
    ) -> Result<SeriesReference> {
        let details = self.tmdb.get_series(tmdb_id).await.map_err(|e| {
            MediaError::Internal(format!("Failed to fetch series details: {e}"))
        })?;

        let credits = self.tmdb.get_series_credits(tmdb_id).await.ok();
        let (content_rating, content_ratings) =
            self.fetch_series_content_rating(tmdb_id).await;

        let genres = details
            .genres
            .iter()
            .map(|g| GenreInfo {
                id: g.id,
                name: g.name.clone(),
            })
            .collect::<Vec<_>>();
        let networks = details
            .networks
            .iter()
            .map(|n| NetworkInfo {
                id: n.id,
                name: n.name.clone(),
                origin_country: n.origin_country.clone().and_then(|country| {
                    let trimmed = country.trim().to_string();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed)
                    }
                }),
            })
            .collect::<Vec<_>>();

        let origin_countries = details.inner.origin_country.clone();

        let spoken_languages = details
            .spoken_languages
            .iter()
            .map(|lang| SpokenLanguage {
                iso_639_1: Some(lang.iso_639_1.clone()),
                name: lang.name.clone(),
            })
            .collect::<Vec<_>>();

        let production_companies = details
            .production_companies
            .iter()
            .map(|company| ProductionCompany {
                id: company.id,
                name: company.name.clone(),
                origin_country: company.origin_country.clone().and_then(
                    |country| {
                        let trimmed = country.trim().to_string();
                        if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed)
                        }
                    },
                ),
            })
            .collect::<Vec<_>>();

        let production_countries = details
            .production_countries
            .iter()
            .map(|country| ProductionCountry {
                iso_3166_1: country.iso_3166_1.clone(),
                name: country.name.clone(),
            })
            .collect::<Vec<_>>();

        let cast = credits
            .as_ref()
            .map(Self::map_series_cast)
            .unwrap_or_default();
        let crew = credits
            .as_ref()
            .map(Self::map_series_crew)
            .unwrap_or_default();

        let homepage = if details.homepage.trim().is_empty() {
            None
        } else {
            Some(details.homepage.clone())
        };

        let enhanced = EnhancedSeriesDetails {
            id: details.inner.id as u64,
            name: details.inner.name.clone(),
            original_name: Some(details.inner.original_name.clone()),
            overview: details.inner.overview.clone(),
            first_air_date: details
                .inner
                .first_air_date
                .as_ref()
                .map(|d| d.to_string()),
            last_air_date: details
                .last_air_date
                .as_ref()
                .map(|d| d.to_string()),
            number_of_seasons: Some(details.number_of_seasons as u32),
            number_of_episodes: details.number_of_episodes.map(|n| n as u32),
            vote_average: Some(details.inner.vote_average as f32),
            vote_count: Some(details.inner.vote_count as u32),
            popularity: Some(details.inner.popularity as f32),
            content_rating,
            content_ratings,
            release_dates: Vec::new(),
            genres,
            networks,
            origin_countries,
            spoken_languages,
            production_companies,
            production_countries,
            homepage,
            status: Some(details.status.clone()),
            tagline: details.tagline.clone(),
            in_production: Some(details.in_production),
            poster_path: details.inner.poster_path.clone(),
            backdrop_path: details.inner.backdrop_path.clone(),
            logo_path: None,
            images: MediaImages::default(),
            cast,
            crew,
            videos: Vec::new(),
            keywords: Vec::new(),
            external_ids: ExternalIds::default(),
            alternative_titles: Vec::new(),
            translations: Vec::new(),
            episode_groups: Vec::new(),
            recommendations: Vec::new(),
            similar: Vec::new(),
        };

        let title =
            SeriesTitle::new(details.inner.name.clone()).map_err(|e| {
                MediaError::Internal(format!(
                    "Invalid series title '{}' ({e})",
                    details.inner.name
                ))
            })?;

        Ok(SeriesReference {
            id: SeriesID::new(),
            library_id,
            tmdb_id,
            title,
            details: MediaDetailsOption::Details(TmdbDetails::Series(enhanced)),
            endpoint: SeriesURL::from_string(format!("/series/{}", tmdb_id)),
            discovered_at: Utc::now(),
            created_at: Utc::now(),
            theme_color: None,
        })
    }

    fn build_series_stub(
        &self,
        library_id: LibraryID,
        info: &EpisodeContextInfo,
        parent: Option<&ParentDescriptors>,
    ) -> Result<SeriesReference> {
        let clean_title = parent
            .and_then(|p| p.series_title_hint.as_deref())
            .map(clean_series_title)
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| info.series.normalized_title.clone());
        let title = SeriesTitle::new(clean_title.clone()).map_err(|e| {
            MediaError::Internal(format!(
                "Invalid series title '{}' ({e})",
                clean_title
            ))
        })?;

        let slug_source = parent
            .and_then(|p| p.series_slug.as_deref())
            .map(|slug| slug.replace('-', " "))
            .unwrap_or_else(|| {
                if info.series.raw_title.is_empty() {
                    clean_title.clone()
                } else {
                    info.series.raw_title.clone()
                }
            });
        let slug = Self::slugify_title(&slug_source);
        let endpoint = format!("/series/lookup/{}", slug);

        Ok(SeriesReference {
            id: SeriesID::new(),
            library_id,
            tmdb_id: 0,
            title,
            details: MediaDetailsOption::Endpoint(endpoint.clone()),
            endpoint: SeriesURL::from_string(endpoint),
            discovered_at: Utc::now(),
            created_at: Utc::now(),
            theme_color: None,
        })
    }

    async fn resolve_season(
        &self,
        library_id: LibraryID,
        series_ref: &SeriesReference,
        season_number: u32,
    ) -> Result<SeasonReference> {
        if let Some(existing) = self
            .media_refs
            .get_series_seasons(&series_ref.id)
            .await?
            .into_iter()
            .find(|season| season.season_number.value() as u32 == season_number)
        {
            return Ok(existing);
        }

        let season_number_u8 = u8::try_from(season_number).map_err(|_| {
            MediaError::InvalidMedia(format!(
                "Season number {} out of range",
                season_number
            ))
        })?;

        let season_details = if series_ref.tmdb_id > 0 {
            match self
                .tmdb
                .get_season(series_ref.tmdb_id, season_number_u8)
                .await
            {
                Ok(details) => Some(details),
                Err(ProviderError::ApiError(msg)) if msg.contains("404") => {
                    return Err(MediaError::InvalidMedia(format!(
                        "{}:{}",
                        SEASON_NOT_FOUND_PREFIX, season_number
                    )));
                }
                Err(err) => {
                    return Err(MediaError::Internal(format!(
                        "Failed to fetch season {} for series {}: {err}",
                        season_number, series_ref.tmdb_id
                    )));
                }
            }
        } else {
            None
        };

        let mut season_ref = self
            .build_season_reference(
                library_id,
                series_ref,
                season_number_u8,
                season_details,
            )
            .await?;

        let actual_id =
            self.media_refs.store_season_reference(&season_ref).await?;

        if season_ref.id.to_uuid() != actual_id {
            season_ref.id = SeasonID(actual_id);
        }

        Ok(season_ref)
    }

    async fn build_season_reference(
        &self,
        library_id: LibraryID,
        series_ref: &SeriesReference,
        season_number: u8,
        season_details: Option<TmdbSeason>,
    ) -> Result<SeasonReference> {
        let mut details_opt = None;
        if let Some(details) = season_details {
            let name = if details.inner.name.trim().is_empty() {
                format!("Season {}", season_number)
            } else {
                details.inner.name.clone()
            };

            details_opt = Some(SeasonDetails {
                id: details.inner.id,
                season_number: details.inner.season_number as u8,
                name,
                overview: details.inner.overview.clone(),
                air_date: details
                    .inner
                    .air_date
                    .as_ref()
                    .map(|d| d.to_string()),
                episode_count: details.episodes.len() as u32,
                poster_path: details.inner.poster_path.clone(),
                runtime: None,
                external_ids: ExternalIds::default(),
                images: MediaImages::default(),
                videos: Vec::new(),
                keywords: Vec::new(),
                translations: Vec::new(),
            });
        }

        let endpoint_path = if series_ref.tmdb_id > 0 {
            format!("/series/{}/season/{}", series_ref.tmdb_id, season_number)
        } else {
            format!("/series/{}/season/{}", series_ref.id, season_number)
        };

        let endpoint = SeasonURL::from_string(endpoint_path.clone());
        let details = match details_opt {
            Some(details) => {
                MediaDetailsOption::Details(TmdbDetails::Season(details))
            }
            None => MediaDetailsOption::Endpoint(endpoint_path.clone()),
        };

        Ok(SeasonReference {
            id: SeasonID::new(),
            library_id,
            season_number: SeasonNumber::new(season_number),
            series_id: series_ref.id,
            tmdb_series_id: series_ref.tmdb_id,
            details,
            endpoint,
            discovered_at: Utc::now(),
            created_at: Utc::now(),
            theme_color: None,
        })
    }

    async fn create_episode_reference(
        &self,
        command: &MetadataCommand,
        series_ref: &SeriesReference,
        season_ref: &SeasonReference,
        metadata: Option<MediaFileMetadata>,
        info: &EpisodeContextInfo,
    ) -> Result<(EpisodeReference, Option<u64>)> {
        let mut media_file = MediaFile::new(
            PathBuf::from(&command.analyzed.path_norm),
            command.analyzed.library_id,
        )?;

        if let Some(meta) = metadata.clone() {
            media_file.media_file_metadata = Some(meta);
        }

        let upsert = self.media_files_write.upsert(media_file.clone()).await?;
        let actual_file_id = upsert.id;
        media_file.id = actual_file_id;

        let episode_number_u8 =
            u8::try_from(info.episode_number).map_err(|_| {
                MediaError::InvalidMedia(format!(
                    "Episode number {} out of range",
                    info.episode_number
                ))
            })?;

        let episode_id = EpisodeID::new();

        let (episode_details, tmdb_episode_id) = if series_ref.tmdb_id > 0 {
            match self
                .tmdb
                .get_episode(
                    series_ref.tmdb_id,
                    season_ref.season_number.value(),
                    episode_number_u8,
                )
                .await
            {
                Ok(details) => {
                    let mapped = EpisodeDetails {
                        id: details.inner.id,
                        episode_number: details.inner.episode_number as u8,
                        season_number: details.inner.season_number as u8,
                        name: details.inner.name.clone(),
                        overview: details.inner.overview.clone(),
                        air_date: details
                            .inner
                            .air_date
                            .as_ref()
                            .map(|d| d.to_string()),
                        runtime: None,
                        still_path: details.inner.still_path.clone(),
                        vote_average: Some(details.inner.vote_average as f32),
                        vote_count: Some(details.inner.vote_count as u32),
                        production_code: if details
                            .inner
                            .production_code
                            .is_empty()
                        {
                            None
                        } else {
                            Some(details.inner.production_code.clone())
                        },
                        external_ids: ExternalIds::default(),
                        images: MediaImages::default(),
                        videos: Vec::new(),
                        keywords: Vec::new(),
                        translations: Vec::new(),
                        guest_stars: Vec::new(),
                        crew: Vec::new(),
                        content_ratings: Vec::new(),
                    };
                    (Some(mapped), Some(details.inner.id))
                }
                Err(ProviderError::ApiError(msg)) if msg.contains("404") => {
                    return Err(MediaError::InvalidMedia(format!(
                        "{}:{}:{}:{}",
                        EPISODE_NOT_FOUND_PREFIX,
                        series_ref.tmdb_id,
                        season_ref.season_number.value(),
                        episode_number_u8
                    )));
                }
                Err(err) => {
                    return Err(MediaError::Internal(format!(
                        "Failed to fetch episode details for series {} S{}E{}: {err}",
                        series_ref.tmdb_id,
                        season_ref.season_number.value(),
                        episode_number_u8
                    )));
                }
            }
        } else {
            (None, None)
        };

        let details = match episode_details {
            Some(details) => {
                MediaDetailsOption::Details(TmdbDetails::Episode(details))
            }
            None => MediaDetailsOption::Endpoint(format!(
                "/episode/lookup/{}",
                actual_file_id
            )),
        };

        let file_discovered_at = media_file.discovered_at;
        let file_created_at = media_file.created_at;

        let episode_ref = EpisodeReference {
            id: episode_id,
            library_id: command.analyzed.library_id,
            episode_number: EpisodeNumber::new(episode_number_u8),
            season_number: season_ref.season_number,
            season_id: season_ref.id,
            series_id: series_ref.id,
            tmdb_series_id: series_ref.tmdb_id,
            details,
            endpoint: EpisodeURL::from_string(format!(
                "/stream/{}",
                actual_file_id
            )),
            file: media_file,
            discovered_at: file_discovered_at,
            created_at: file_created_at,
        };

        self.media_refs
            .store_episode_reference(&episode_ref)
            .await?;

        Ok((episode_ref, tmdb_episode_id))
    }

    fn annotate_episode_context(
        context: &mut Value,
        series_ref: &SeriesReference,
        season_ref: &SeasonReference,
        episode_ref: &EpisodeReference,
        info: &EpisodeContextInfo,
        tmdb_episode_id: Option<u64>,
    ) {
        let map = Self::ensure_context_object(context);

        let mut parent = map
            .get("parent")
            .cloned()
            .and_then(|raw| {
                serde_json::from_value::<ParentDescriptors>(raw).ok()
            })
            .unwrap_or_default();
        parent.series_id = Some(series_ref.id);
        parent.season_id = Some(season_ref.id);
        parent.episode_id = Some(episode_ref.id);
        parent.resolved_type = Some(LibraryType::Series);
        if let Ok(parent_json) = serde_json::to_value(parent) {
            map.insert("parent".into(), parent_json);
        }

        map.insert(
            "series_id".into(),
            Value::String(series_ref.id.to_string()),
        );
        map.insert(
            "season_id".into(),
            Value::String(season_ref.id.to_string()),
        );
        map.insert(
            "episode_id".into(),
            Value::String(episode_ref.id.to_string()),
        );
        map.insert(
            "series_title".into(),
            Value::String(series_ref.title.as_str().to_string()),
        );
        map.insert(
            "season_number".into(),
            Value::from(season_ref.season_number.value() as u64),
        );
        map.insert(
            "episode_number".into(),
            Value::from(episode_ref.episode_number.value() as u64),
        );
        map.insert("tmdb_series_id".into(), Value::from(series_ref.tmdb_id));
        if let Some(tmdb_episode_id) = tmdb_episode_id {
            map.insert("tmdb_episode_id".into(), Value::from(tmdb_episode_id));
        }
        if let Some(title) = info.episode_title.as_ref() {
            map.insert("episode_title".into(), Value::String(title.clone()));
        }
    }

    fn build_episode_normalized_title(
        series_ref: &SeriesReference,
        season_ref: &SeasonReference,
        episode_ref: &EpisodeReference,
        info: &EpisodeContextInfo,
    ) -> String {
        let base = format!(
            "{} S{:02}E{:02}",
            series_ref.title.as_str(),
            season_ref.season_number.value(),
            episode_ref.episode_number.value()
        );

        if let Some(title) = info.episode_title.as_ref() {
            format!("{} {}", base, title.trim())
        } else {
            base
        }
    }

    fn pick_series_candidate<'a>(
        query: &str,
        _region: Option<&str>,
        results: &'a [SeriesReference],
    ) -> Option<&'a SeriesReference> {
        if results.is_empty() {
            return None;
        }

        let query_lower = query.to_lowercase();
        if let Some(exact) = results
            .iter()
            .find(|res| res.title.as_str().to_lowercase() == query_lower)
        {
            return Some(exact);
        }

        results.first()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tmdb_api::tvshow::content_rating::ContentRating as TmdbContentRating;

    #[test]
    fn map_series_content_ratings_normalizes_and_dedupes() {
        let data = TvContentRatingResult {
            id: 42,
            results: vec![
                TmdbContentRating {
                    iso_3166_1: "AU ".to_string(),
                    rating: " R 18+ ".to_string(),
                    descriptors: vec![" Strong Violence ".to_string()],
                },
                TmdbContentRating {
                    iso_3166_1: "au".to_string(),
                    rating: "R18+".to_string(),
                    descriptors: vec![String::new()],
                },
                TmdbContentRating {
                    iso_3166_1: "US".to_string(),
                    rating: String::new(),
                    descriptors: vec![],
                },
                TmdbContentRating {
                    iso_3166_1: "US".to_string(),
                    rating: "TV-MA".to_string(),
                    descriptors: vec!["drugs".to_string()],
                },
            ],
        };

        let mapped = TmdbMetadataActor::map_series_content_ratings(&data);

        assert_eq!(mapped.len(), 2);

        let au = mapped.iter().find(|r| r.iso_3166_1 == "AU").unwrap();
        assert_eq!(au.rating.as_deref(), Some("R18+"));
        assert_eq!(au.descriptors, vec!["Strong Violence".to_string()]);

        let us = mapped.iter().find(|r| r.iso_3166_1 == "US").unwrap();
        assert_eq!(us.rating.as_deref(), Some("TV-MA"));
        assert_eq!(us.descriptors, vec!["drugs".to_string()]);
    }
}

#[derive(Debug)]
enum MetadataMediaKind {
    Movie,
    Episode,
    Unknown,
}

#[async_trait]
impl MetadataActor for TmdbMetadataActor {
    async fn enrich(
        &self,
        command: MetadataCommand,
    ) -> Result<MediaReadyForIndex> {
        match Self::infer_media_kind(&command.analyzed.context) {
            MetadataMediaKind::Movie => self.enrich_movie(command).await,
            MetadataMediaKind::Episode => self.enrich_episode(command).await,
            MetadataMediaKind::Unknown => {
                DefaultMetadataActor::new().enrich(command).await
            }
        }
    }
}
