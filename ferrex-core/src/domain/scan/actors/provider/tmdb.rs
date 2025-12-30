use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use chrono::Utc;

use ferrex_model::{
    BackdropSize, ImageMediaType, ImageSize, MediaID, PosterSize,
    VideoMediaType,
};
use once_cell::sync::Lazy;
use regex::Regex;
use tracing::{error, warn};

use super::tmdb_match::{rank_movie_candidates, rank_series_candidates};

use crate::{
    database::repository_ports::{
        images::VarInput, media_files::MediaFilesWritePort,
        media_references::MediaReferencesRepository,
    },
    domain::media::tv_parser::TvParser,
    error::{MediaError, Result},
    infra::media::{
        image_service::ImageService,
        providers::{ProviderError, TmdbApiProvider},
    },
    traits::prelude::MediaIDLike,
    types::{
        details::{
            AlternativeTitle, CastMember, CollectionInfo, ContentRating,
            CrewMember, EnhancedMovieDetails, EnhancedSeriesDetails,
            EpisodeDetails, ExternalIds, GenreInfo, Keyword, NetworkInfo,
            PersonExternalIds, ProductionCompany, ProductionCountry,
            RelatedMediaRef, ReleaseDateEntry, ReleaseDatesByCountry,
            SeasonDetails, SpokenLanguage, Translation, Video,
        },
        files::{MediaFile, MediaFileMetadata, ParsedMediaInfo},
        ids::{EpisodeID, LibraryId, MovieID, SeasonID, SeriesID},
        image::MediaImages,
        media::{EpisodeReference, MovieReference, SeasonReference, Series},
        numbers::{EpisodeNumber, SeasonNumber},
        titles::{MovieTitle, SeriesTitle},
        urls::{EpisodeURL, MovieURL, SeasonURL, SeriesURL, UrlLike},
    },
};

use crate::domain::scan::actors::metadata::{
    DefaultMetadataActor, MediaReadyForIndex, MetadataActor, MetadataCommand,
};
use crate::domain::scan::orchestration::context::{
    EpisodeLink, EpisodeRef, ScanNodeKind, SeasonLink, SeasonRef, SeriesHint,
    SeriesLink, SeriesRef, SeriesRootPath, SeriesScanHierarchy,
};
use crate::domain::scan::{
    AnalyzeScanHierarchy, MediaFingerprint,
    analyze::MediaAnalyzed,
    orchestration::{
        job::{ImageFetchJob, ImageFetchPriority, ScanReason},
        series::{
            SeriesFolderClues, SeriesLocator, SeriesMetadataProvider,
            SeriesResolution, clean_series_title, collapse_whitespace,
        },
    },
};
use tmdb_api::{
    common::release_date::ReleaseDateKind,
    movie::{
        alternative_titles::Response as MovieAltTitleResponse,
        credits::GetMovieCreditsResponse, external_ids::MovieExternalIds,
        images::GetMovieImagesResponse, keywords::Response as KeywordsResponse,
        release_dates::Response as ReleaseDatesResponse,
        translations::Response as TranslationResponse,
        videos::Response as MovieVideosResponse,
    },
    tvshow::{
        Season as TmdbSeason, aggregate_credits::TVShowAggregateCredits,
        content_rating::Response as SeriesContentRatingResponse,
    },
};
use uuid::Uuid;

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
    season_number: u16,
    episode_number: u16,
    episode_title: Option<String>,
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

    /// Persist TMDB image variants for a movie into tmdb_image_variants.
    /// Maps posters and backdrops from the TMDB images response into VarInput rows.
    async fn persist_tmdb_variants_movie<'a>(
        &'a self,
        media_id: Uuid,
        media_type: ImageMediaType,
        images: &'a GetMovieImagesResponse,
    ) -> Result<Vec<VarInput<'a>>> {
        #[derive(Clone)]
        struct TmpVar<'a> {
            tmdb_path: &'a str,
            imz: ImageSize,
            width: i16,
            height: i16,
            lang: &'a str,
            v_avg: f32,
            v_cnt: u32,
            is_primary: bool,
        }

        fn mark_primary(candidates: &mut [TmpVar<'_>]) {
            if candidates.is_empty() {
                return;
            }
            let mut counts: Vec<u32> =
                candidates.iter().map(|c| c.v_cnt).collect();
            counts.sort_unstable();
            let median = counts[counts.len() / 2];
            let primary_idx = candidates
                .iter()
                .enumerate()
                .filter(|(_, c)| c.v_cnt > median)
                .max_by(|(_, a), (_, b)| {
                    a.v_avg
                        .partial_cmp(&b.v_avg)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i);
            let chosen = primary_idx.or_else(|| {
                candidates
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| {
                        a.v_cnt.cmp(&b.v_cnt).then_with(|| {
                            a.v_avg
                                .partial_cmp(&b.v_avg)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                    })
                    .map(|(idx, _)| idx)
            });

            if let Some(idx) = chosen {
                candidates[idx].is_primary = true;
            }
        }

        let mut poster_vars: Vec<TmpVar<'_>> = images
            .posters
            .iter()
            .map(|poster| TmpVar {
                tmdb_path: &poster.file_path,
                imz: ImageSize::Poster(PosterSize::original(
                    poster.width as u32,
                )),
                width: poster.width as i16,
                height: poster.height as i16,
                lang: poster.iso_639_1.as_deref().unwrap_or(""),
                v_avg: poster.vote_average as f32,
                v_cnt: poster.vote_count as u32,
                is_primary: false,
            })
            .collect();

        let mut backdrop_vars: Vec<TmpVar<'_>> = images
            .backdrops
            .iter()
            .map(|backdrop| TmpVar {
                tmdb_path: &backdrop.file_path,
                imz: ImageSize::Backdrop(BackdropSize::original(
                    backdrop.width as u32,
                )),
                width: backdrop.width as i16,
                height: backdrop.height as i16,
                lang: backdrop.iso_639_1.as_deref().unwrap_or(""),
                v_avg: backdrop.vote_average as f32,
                v_cnt: backdrop.vote_count as u32,
                is_primary: false,
            })
            .collect();

        mark_primary(&mut poster_vars);
        mark_primary(&mut backdrop_vars);

        let mut variants =
            Vec::with_capacity(poster_vars.len() + backdrop_vars.len());
        for v in poster_vars.into_iter().chain(backdrop_vars.into_iter()) {
            variants.push(VarInput {
                media_id,
                media_type,
                tmdb_path: v.tmdb_path,
                imz: v.imz,
                width: v.width,
                height: v.height,
                lang: v.lang,
                v_avg: v.v_avg,
                v_cnt: v.v_cnt,
                is_primary: v.is_primary,
            });
        }

        Ok(variants)
    }

    /// Persist TMDB image variants for a series into tmdb_image_variants.
    /// Maps posters and backdrops from the TMDB images response into VarInput rows.
    async fn persist_tmdb_variants_series<'a>(
        &'a self,
        media_id: Uuid,
        media_type: ImageMediaType,
        images: &'a tmdb_api::tvshow::images::GetTVshowImagesResponse,
    ) -> Result<Vec<VarInput<'a>>> {
        #[derive(Clone)]
        struct TmpVar<'a> {
            tmdb_path: &'a str,
            imz: ImageSize,
            width: i16,
            height: i16,
            lang: &'a str,
            v_avg: f32,
            v_cnt: u32,
            is_primary: bool,
        }

        fn mark_primary(candidates: &mut [TmpVar<'_>]) {
            if candidates.is_empty() {
                return;
            }
            let mut counts: Vec<u32> =
                candidates.iter().map(|c| c.v_cnt).collect();
            counts.sort_unstable();
            let median = counts[counts.len() / 2];
            let primary_idx = candidates
                .iter()
                .enumerate()
                .filter(|(_, c)| c.v_cnt > median)
                .max_by(|(_, a), (_, b)| {
                    a.v_avg
                        .partial_cmp(&b.v_avg)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i);
            let chosen = primary_idx.or_else(|| {
                candidates
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| {
                        a.v_cnt.cmp(&b.v_cnt).then_with(|| {
                            a.v_avg
                                .partial_cmp(&b.v_avg)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                    })
                    .map(|(idx, _)| idx)
            });

            if let Some(idx) = chosen {
                candidates[idx].is_primary = true;
            }
        }

        let mut poster_vars: Vec<TmpVar<'_>> = images
            .posters
            .iter()
            .map(|poster| TmpVar {
                tmdb_path: &poster.file_path,
                imz: ImageSize::Poster(PosterSize::original(
                    poster.width as u32,
                )),
                width: poster.width as i16,
                height: poster.height as i16,
                lang: poster.iso_639_1.as_deref().unwrap_or(""),
                v_avg: poster.vote_average as f32,
                v_cnt: poster.vote_count as u32,
                is_primary: false,
            })
            .collect();

        let mut backdrop_vars: Vec<TmpVar<'_>> = images
            .backdrops
            .iter()
            .map(|backdrop| TmpVar {
                tmdb_path: &backdrop.file_path,
                imz: ImageSize::Backdrop(BackdropSize::from_width(
                    backdrop.width as u32,
                )),
                width: backdrop.width as i16,
                height: backdrop.height as i16,
                lang: backdrop.iso_639_1.as_deref().unwrap_or(""),
                v_avg: backdrop.vote_average as f32,
                v_cnt: backdrop.vote_count as u32,
                is_primary: false,
            })
            .collect();

        mark_primary(&mut poster_vars);
        mark_primary(&mut backdrop_vars);

        let mut variants =
            Vec::with_capacity(poster_vars.len() + backdrop_vars.len());
        for v in poster_vars.into_iter().chain(backdrop_vars.into_iter()) {
            variants.push(VarInput {
                media_id,
                media_type,
                tmdb_path: v.tmdb_path,
                imz: v.imz,
                width: v.width,
                height: v.height,
                lang: v.lang,
                v_avg: v.v_avg,
                v_cnt: v.v_cnt,
                is_primary: v.is_primary,
            });
        }

        Ok(variants)
    }

    async fn resolve_series_from_hint(
        &self,
        library_id: LibraryId,
        hint: &SeriesHint,
    ) -> Result<Series> {
        let search_results = self
            .tmdb
            .search_series(
                &hint.title,
                hint.year,
                None, // TODO: Pass through library language
                hint.region.as_deref(),
            )
            .await
            .map_err(|e| {
                MediaError::Internal(format!("TMDB series search failed: {e}"))
            })?;

        let ranked = rank_series_candidates(
            &hint.title,
            hint.year,
            &search_results.results,
        );
        tracing::debug!(
            query_title = %hint.title,
            query_year = ?hint.year,
            candidates = ranked.len(),
            "TMDB series candidates ranked"
        );
        for entry in ranked.iter().take(3) {
            let title = entry.candidate.inner.name.as_str();
            tracing::debug!(
                tmdb_id = entry.candidate.inner.id,
                title = %title,
                has_poster = entry.rank.has_poster,
                title_exact = entry.rank.title.exact_normalized,
                title_overlap_bp = entry.rank.title.overlap_bp,
                title_jaccard_bp = entry.rank.title.jaccard_bp,
                year_rank = ?entry.rank.year,
                vote_count = entry.rank.vote_count,
                popularity = %entry.rank.popularity,
                "TMDB series candidate"
            );
        }

        if let Some(primary) = ranked.first().map(|entry| entry.candidate) {
            let tmdb_id = primary.inner.id;
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
            return Ok(series_ref);
        }

        Err(MediaError::InvalidMedia(format!(
            "series_not_found: {}",
            hint.title
        )))
    }

    fn extract_technical_metadata(
        analysis: &crate::domain::scan::actors::analyze::AnalysisContext,
    ) -> Option<MediaFileMetadata> {
        analysis.technical.clone()
    }

    async fn enrich_series(
        &self,
        command: MetadataCommand,
    ) -> Result<MediaReadyForIndex> {
        let series_id = match command.job.media_id {
            MediaID::Series(id) => id,
            _ => {
                return Err(MediaError::InvalidMedia(
                    "series enrich requires series media id".into(),
                ));
            }
        };

        let series_ref =
            self.media_refs.get_series_reference(&series_id).await?;
        let AnalyzeScanHierarchy::Series(mut hierarchy) =
            command.job.hierarchy.clone()
        else {
            return Err(MediaError::InvalidMedia(
                "series enrich requires series scan hierarchy".into(),
            ));
        };

        hierarchy.series = SeriesLink::Resolved(SeriesRef {
            id: series_ref.id,
            slug: None,
            title: Some(series_ref.title.as_str().to_string()),
        });

        Ok(MediaReadyForIndex {
            library_id: command.job.library_id,
            media_id: command.job.media_id,
            variant: command.job.variant,
            hierarchy: AnalyzeScanHierarchy::Series(hierarchy),
            node: command.job.node.clone(),
            normalized_title: Some(series_ref.title.as_str().to_string()),
            analyzed: command.analyzed,
            prepared_at: Utc::now(),
            image_jobs: vec![],
        })
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
            });
        }

        TvParser::parse_episode_info(path).map(|info| {
            let clues = folder_clues;
            let episode_title = TvParser::extract_episode_title(path);
            EpisodeContextInfo {
                series: clues,
                season_number: info.season,
                episode_number: info.episode,
                episode_title,
            }
        })
    }

    async fn queue_local_episode_thumbnail(
        &self,
        library_id: LibraryId,
        episode: &mut EpisodeReference,
        jobs: &mut Vec<ImageFetchJob>,
    ) -> Result<()> {
        #[cfg(feature = "demo")]
        if crate::domain::demo::policy()
            .is_some_and(|policy| policy.skip_episode_thumbnails)
            && crate::domain::demo::is_demo_library(&library_id)
        {
            return Ok(());
        }

        let iid = episode.details.primary_still_iid.unwrap_or(Uuid::now_v7());
        let imz = ImageSize::thumbnail();

        if self
            .image_service
            .images
            .lookup_variant_by_iid(iid)
            .await?
            .is_none()
        {
            let (width, height) = imz.dimensions().ok_or_else(|| {
                MediaError::Internal(
                    "Episode thumbnail size requires explicit dimensions"
                        .into(),
                )
            })?;

            let tmdb_path = Self::local_episode_thumbnail_path(iid);

            self.image_service
                .images
                .upsert_variant(&VarInput {
                    media_id: episode.id.to_uuid(),
                    media_type: ImageMediaType::Episode,
                    tmdb_path: &tmdb_path,
                    imz,
                    width: width as i16,
                    height: height as i16,
                    lang: "",
                    v_avg: 0.0,
                    v_cnt: 0,
                    is_primary: true,
                })
                .await?;
        }

        jobs.push(ImageFetchJob {
            library_id,
            iid,
            imz,
            priority_hint: ImageFetchPriority::Backdrop,
        });

        episode.details.primary_still_iid = Some(iid);

        Ok(())
    }

    fn local_episode_thumbnail_path(iid: Uuid) -> String {
        format!("local-ep-{}", iid)
    }

    async fn queue_person_profile_jobs(
        &self,
        library_id: LibraryId,
        cast: &[CastMember],
        jobs: &mut Vec<ImageFetchJob>,
    ) -> Result<()> {
        let mut seen_people = HashSet::new();

        for member in cast {
            if !seen_people.insert(member.id) {
                continue;
            }

            let has_profile_path = member
                .profile_path
                .as_deref()
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .is_some();
            if !has_profile_path {
                continue;
            }

            let Some(iid) = member.image_id else {
                warn!("Missing image id for cast member {}", member.id);
                continue;
            };

            jobs.push(ImageFetchJob {
                library_id,
                iid,
                imz: ImageSize::profile(),
                priority_hint: ImageFetchPriority::Profile,
            });
        }

        Ok(())
    }

    #[allow(dead_code)]
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

    fn is_invalid_series_title(title: &str) -> bool {
        let trimmed = clean_series_title(title);
        if trimmed.is_empty() {
            return true;
        }
        let lowered = trimmed.to_ascii_lowercase();
        if lowered == "unknown series"
            || lowered == "extras"
            || lowered == "special"
            || lowered == "specials"
        {
            return true;
        }
        // Treat season-like tokens as invalid standalone series titles
        if crate::domain::media::tv_parser::TvParser::parse_season_folder(
            &trimmed,
        )
        .is_some()
        {
            return true;
        }
        false
    }

    fn handle_movie_release_dates(
        tmdb_id: u64,
        result: std::result::Result<ReleaseDatesResponse, ProviderError>,
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
        data: &ReleaseDatesResponse,
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
        data: &ReleaseDatesResponse,
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
        data: &ReleaseDatesResponse,
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

    fn map_movie_keywords(result: &KeywordsResponse) -> Vec<Keyword> {
        result
            .keywords
            .iter()
            .map(|keyword| Keyword {
                id: keyword.id,
                name: keyword.name.clone(),
            })
            .collect()
    }

    fn map_movie_videos(result: &MovieVideosResponse) -> Vec<Video> {
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
        result: &TranslationResponse,
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
        result: &MovieAltTitleResponse,
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

    fn map_movie_external_ids(result: &MovieExternalIds) -> ExternalIds {
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

    fn map_cast(credits: &GetMovieCreditsResponse) -> Vec<CastMember> {
        let mut next_slot: u32 = 0;
        credits
            .cast
            .iter()
            .take(20)
            .map(|c| {
                let slot = if c.person.profile_path.is_some() {
                    let assigned = next_slot;
                    next_slot = next_slot.saturating_add(1);
                    assigned
                } else {
                    u32::MAX
                };

                CastMember {
                    id: c.person.id,
                    person_id: None,
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
                    image_id: None,
                }
            })
            .collect()
    }

    fn map_crew(credits: &GetMovieCreditsResponse) -> Vec<CrewMember> {
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
                person_id: None,
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
                profile_iid: None,
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
        data: &SeriesContentRatingResponse,
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
        data: &SeriesContentRatingResponse,
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

    fn map_series_cast(credits: &TVShowAggregateCredits) -> Vec<CastMember> {
        let mut next_slot: u32 = 0;
        credits
            .cast
            .iter()
            .take(20)
            .map(|c| {
                let slot = if c.inner.profile_path.is_some() {
                    let assigned = next_slot;
                    next_slot = next_slot.saturating_add(1);
                    assigned
                } else {
                    u32::MAX
                };

                CastMember {
                    id: c.inner.id,
                    person_id: None,
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
                    image_id: None,
                }
            })
            .collect()
    }

    fn map_series_crew(credits: &TVShowAggregateCredits) -> Vec<CrewMember> {
        credits
            .crew
            .iter()
            .take(20)
            .map(|c| CrewMember {
                id: c.inner.id,
                person_id: None,
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
                profile_iid: None,
            })
            .collect()
    }

    async fn enrich_movie(
        &self,
        mut command: MetadataCommand,
    ) -> Result<MediaReadyForIndex> {
        let metadata =
            Self::extract_technical_metadata(&command.analyzed.analysis);
        let path = PathBuf::from(&command.analyzed.path_norm);
        let (title_hint, year_hint) =
            Self::derive_movie_info(metadata.as_ref(), &path);
        let clean_title = clean_series_title(&title_hint);

        // Search strategy:
        // - Prefer passing year_hint through to TMDB (reduces noise for common titles)
        // - If no acceptable candidates found and we *did* pass a year, retry without year
        //   to tolerate bad filename years (e.g., "Dune Part Two (2023)" vs TMDB 2024).
        #[derive(Debug, Clone, Copy)]
        enum MovieSearchMode {
            WithYear,
            WithoutYearFallback,
        }

        let search_plan: &[MovieSearchMode] = if year_hint.is_some() {
            &[
                MovieSearchMode::WithYear,
                MovieSearchMode::WithoutYearFallback,
            ]
        } else {
            &[MovieSearchMode::WithYear]
        };

        let mut last_missing_poster: Option<String> = None;
        let mut attempted: HashSet<u64> = HashSet::new();
        let mut tried_without_year = false;

        for mode in search_plan {
            let (search_year, label) = match mode {
                MovieSearchMode::WithYear => (year_hint, "with_year"),
                MovieSearchMode::WithoutYearFallback => {
                    tried_without_year = true;
                    (None, "without_year_fallback")
                }
            };

            // TODO: Thread language and region through
            let search_results = self
                .tmdb
                .search_movies(&clean_title, search_year, None, None)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!("TMDB search failed: {e}"))
                })?;

            let ranked = rank_movie_candidates(
                &clean_title,
                year_hint,
                &search_results.results,
            );

            tracing::debug!(
                query_title = %clean_title,
                query_year = ?year_hint,
                tmdb_year_param = ?search_year,
                search_mode = %label,
                candidates = ranked.len(),
                "TMDB movie candidates ranked"
            );

            for entry in ranked.iter().take(3) {
                let title = entry.candidate.inner.title.as_str();
                tracing::debug!(
                    tmdb_id = entry.candidate.inner.id,
                    title = %title,
                    has_poster = entry.rank.has_poster,
                    title_exact = entry.rank.title.exact_normalized,
                    title_overlap_bp = entry.rank.title.overlap_bp,
                    title_jaccard_bp = entry.rank.title.jaccard_bp,
                    year_rank = ?entry.rank.year,
                    vote_count = entry.rank.vote_count,
                    popularity = %entry.rank.popularity,
                    "TMDB movie candidate"
                );
            }

            for entry in ranked {
                let tmdb_id = entry.candidate.inner.id;
                if !attempted.insert(tmdb_id) {
                    continue;
                }

                if !entry.rank.is_acceptable() {
                    continue;
                }

                let movie_ref = match self
                    .build_movie_reference(
                        command.analyzed.library_id,
                        &command.analyzed.path_norm,
                        metadata.as_ref(),
                        tmdb_id,
                    )
                    .await
                {
                    Ok(movie_ref) => movie_ref,
                    Err(MediaError::InvalidMedia(msg))
                        if msg.starts_with("missing_primary_poster:movie:") =>
                    {
                        last_missing_poster = Some(msg);
                        continue;
                    }
                    Err(err) => return Err(err),
                };

                let media_id =
                    self.media_refs.store_movie_reference(&movie_ref).await?;
                let movie_id = match media_id {
                    MediaID::Movie(id) => id,
                    other => {
                        return Err(MediaError::Internal(format!(
                            "Expected movie id after movie store, got {:?}",
                            other
                        )));
                    }
                };
                let stored_movie =
                    self.media_refs.get_movie_reference(&movie_id).await?;

                let library_id = command.job.library_id;

                let primary_poster_iid =
                    movie_ref.details.primary_poster_iid.ok_or_else(|| {
                        MediaError::Internal(format!(
                            "TMDB movie reference missing primary poster iid (tmdb_id={tmdb_id})"
                        ))
                    })?;

                let mut image_jobs = Self::movie_primary_image_jobs(
                    library_id,
                    primary_poster_iid,
                    movie_ref.details.primary_backdrop_iid,
                );

                self.queue_person_profile_jobs(
                    library_id,
                    &stored_movie.details.cast,
                    &mut image_jobs,
                )
                .await?;

                command.analyzed.analysis.tmdb_id_hint = Some(tmdb_id);
                let AnalyzeScanHierarchy::Movie(ref mut hierarchy) =
                    command.analyzed.hierarchy
                else {
                    return Err(MediaError::Internal(
                        "tmdb movie enrich requires movie hierarchy".into(),
                    ));
                };
                hierarchy.movie_id = Some(movie_id);

                return Ok(MediaReadyForIndex {
                    library_id: command.job.library_id,
                    media_id,
                    variant: command.analyzed.variant,
                    hierarchy: command.analyzed.hierarchy.clone(),
                    node: command.analyzed.node.clone(),
                    normalized_title: Some(movie_ref.title.to_string()),
                    analyzed: command.analyzed,
                    prepared_at: Utc::now(),
                    image_jobs,
                });
            }
        }

        if let Some(msg) = last_missing_poster {
            return Err(MediaError::InvalidMedia(msg));
        }

        if tried_without_year && year_hint.is_some() {
            tracing::info!(
                query_title = %clean_title,
                query_year = ?year_hint,
                "No acceptable TMDB candidates found with year filter; also tried without year"
            );
        }

        Err(MediaError::NotFound(format!(
            "Movie match not found (title={:#?}, year={:?})",
            clean_title, year_hint
        )))
    }

    fn movie_primary_image_jobs(
        library_id: LibraryId,
        primary_poster_iid: Uuid,
        primary_backdrop_iid: Option<Uuid>,
    ) -> Vec<ImageFetchJob> {
        let mut jobs = Vec::with_capacity(2);
        jobs.push(ImageFetchJob {
            library_id,
            iid: primary_poster_iid,
            imz: ImageSize::poster(),
            priority_hint: ImageFetchPriority::Poster,
        });

        if let Some(iid) = primary_backdrop_iid {
            jobs.push(ImageFetchJob {
                library_id,
                iid,
                imz: ImageSize::backdrop(),
                priority_hint: ImageFetchPriority::Backdrop,
            });
        }

        jobs
    }

    async fn build_movie_reference(
        &self,
        library_id: LibraryId,
        path_norm: &str,
        metadata: Option<&MediaFileMetadata>,
        tmdb_id: u64,
    ) -> Result<MovieReference> {
        // TODO: Passthrough language and region preference
        let tmdb_details =
            self.tmdb.get_movie(tmdb_id, None).await.map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to fetch movie details: {e}"
                ))
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
            images_res,
        ) = tokio::join!(
            self.tmdb.get_movie_release_dates(tmdb_id),
            self.tmdb.get_movie_keywords(tmdb_id),
            self.tmdb.get_movie_videos(tmdb_id, None),
            self.tmdb.get_movie_translations(tmdb_id),
            self.tmdb.get_movie_alternative_titles(tmdb_id, None),
            self.tmdb.get_movie_recommendations(tmdb_id, None, None),
            self.tmdb.get_movie_similar(tmdb_id, None, None),
            self.tmdb.get_movie_external_ids(tmdb_id),
            self.tmdb.get_movie_credits(tmdb_id, None),
            self.tmdb.get_movie_images(tmdb_id, None),
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

        // let (cast, crew) = credits_res
        //     .map(|res| Self::map_movie_credits(&res))
        //     .unwrap_or_else(|err| {
        //         warn!("Failed to fetch movie credits for {}: {}", tmdb_id, err);
        //         (vec![], vec![])
        //     });

        let credits = credits_res.ok();
        // let images = images_res.ok();

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

        let allow_zero_length = {
            #[cfg(feature = "demo")]
            {
                crate::domain::demo::allow_zero_length_for(&library_id)
            }
            #[cfg(not(feature = "demo"))]
            {
                false
            }
        };

        let movie_id = MovieID::new();

        let mut media_file = MediaFile::new_with_policy(
            MediaID::Movie(movie_id),
            PathBuf::from(path_norm),
            library_id,
            allow_zero_length,
        )?;
        if let Some(meta) = metadata {
            media_file.media_file_metadata = Some(meta.clone());
        }

        let upsert = self.media_files_write.upsert(media_file.clone()).await?;
        let actual_file_id = upsert.id;
        media_file.id = actual_file_id;

        let mut cast = credits.as_ref().map(Self::map_cast).unwrap_or_default();
        let mut crew = credits.as_ref().map(Self::map_crew).unwrap_or_default();

        // Persist person profile variants so the player can request cast/crew
        // portraits by `tmdb_image_variants.id` directly.
        self.persist_person_profile_variants(&mut cast, &mut crew)
            .await?;

        let mut primary_poster_iid: Option<Uuid> = None;
        let mut primary_backdrop_iid: Option<Uuid> = None;
        match images_res {
            Ok(images) => {
                let variants = self
                    .persist_tmdb_variants_movie(
                        movie_id.to_uuid(),
                        ImageMediaType::Movie,
                        &images,
                    )
                    .await?;
                let inserted = self
                    .image_service
                    .images
                    .upsert_variants(&variants)
                    .await?;

                fn pick_primary_iid(
                    variants: &[crate::database::traits::OriginalImage],
                    kind: fn(&ImageSize) -> bool,
                ) -> Option<Uuid> {
                    use std::cmp::Ordering;
                    variants
                        .iter()
                        .filter(|v| kind(&v.imz))
                        .max_by(|a, b| {
                            a.is_primary
                                .cmp(&b.is_primary)
                                .then_with(|| {
                                    a.vote_cnt.cmp(&b.vote_cnt).then_with(
                                        || {
                                            a.vote_avg
                                                .partial_cmp(&b.vote_avg)
                                                .unwrap_or(Ordering::Equal)
                                        },
                                    )
                                })
                                .then_with(|| {
                                    a.imz
                                        .width()
                                        .unwrap_or(0)
                                        .cmp(&b.imz.width().unwrap_or(0))
                                })
                        })
                        .map(|v| v.iid)
                }

                primary_poster_iid = pick_primary_iid(&inserted, |imz| {
                    matches!(imz, ImageSize::Poster(_))
                });
                primary_backdrop_iid = pick_primary_iid(&inserted, |imz| {
                    matches!(imz, ImageSize::Backdrop(_))
                });
            }
            Err(e) => error!("Movie images fetch failed with error: {:#?}", e),
        }

        if primary_poster_iid.is_none()
            && let Some(path) = tmdb_details
                .inner
                .poster_path
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
        {
            let variant = self
                .image_service
                .images
                .upsert_variant(&VarInput {
                    media_id: movie_id.to_uuid(),
                    media_type: ImageMediaType::Movie,
                    tmdb_path: path,
                    imz: ImageSize::poster(),
                    width: 0,
                    height: 0,
                    lang: "",
                    v_avg: 0.0,
                    v_cnt: 0,
                    is_primary: true,
                })
                .await?;
            primary_poster_iid = Some(variant.iid);
        }

        if primary_backdrop_iid.is_none()
            && let Some(path) = tmdb_details
                .inner
                .backdrop_path
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
        {
            let variant = self
                .image_service
                .images
                .upsert_variant(&VarInput {
                    media_id: movie_id.to_uuid(),
                    media_type: ImageMediaType::Movie,
                    tmdb_path: path,
                    imz: ImageSize::backdrop(),
                    width: 0,
                    height: 0,
                    lang: "",
                    v_avg: 0.0,
                    v_cnt: 0,
                    is_primary: true,
                })
                .await?;
            primary_backdrop_iid = Some(variant.iid);
        }

        let Some(primary_poster_iid) = primary_poster_iid else {
            return Err(MediaError::InvalidMedia(format!(
                "missing_primary_poster:movie:{}:{}",
                tmdb_id, tmdb_details.inner.title
            )));
        };

        if primary_backdrop_iid.is_none() {
            warn!(
                "missing_primary_backdrop:movie:{}:{}",
                tmdb_id, tmdb_details.inner.title
            );
        };

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
            primary_poster_iid: Some(primary_poster_iid),
            primary_backdrop_iid,
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
            batch_id: None,
            tmdb_id,
            title: MovieTitle::new(tmdb_details.inner.title.clone()).map_err(
                |e| MediaError::Internal(format!("Invalid movie title: {e}")),
            )?,
            details: enhanced,
            endpoint: MovieURL::from_string(format!(
                "/stream/{actual_file_id}"
            )),
            file: media_file,
            theme_color: None,
        };

        Ok(movie_ref)
    }

    // async fn store_movie_without_tmdb(
    //     &self,
    //     mut command: MetadataCommand,
    //     metadata: Option<MediaFileMetadata>,
    // ) -> Result<MediaReadyForIndex> {
    //     let allow_zero_length = {
    //         #[cfg(feature = "demo")]
    //         {
    //             crate::demo::allow_zero_length_for(&command.analyzed.library_id)
    //         }
    //         #[cfg(not(feature = "demo"))]
    //         {
    //             false
    //         }
    //     };

    //     let mut media_file = MediaFile::new_with_policy(
    //         PathBuf::from(&command.analyzed.path_norm),
    //         command.analyzed.library_id,
    //         allow_zero_length,
    //     )?;
    //     if let Some(meta) = metadata.clone() {
    //         media_file.media_file_metadata = Some(meta);
    //     }

    //     let title = metadata
    //         .as_ref()
    //         .and_then(|meta| match &meta.parsed_info {
    //             Some(ParsedMediaInfo::Movie(info)) => Some(info.title.clone()),
    //             _ => None,
    //         })
    //         .unwrap_or_else(|| {
    //             Path::new(&command.analyzed.path_norm)
    //                 .file_stem()
    //                 .and_then(|s| s.to_str())
    //                 .unwrap_or_default()
    //                 .replace(['.', '_', '-'], " ")
    //         });

    //     let movie_id = MovieID::new();
    //     let movie_ref = MovieReference {
    //         id: movie_id,
    //         library_id: command.analyzed.library_id,
    //         tmdb_id: 0,
    //         title: MovieTitle::new(title.clone()).map_err(|e| {
    //             MediaError::Internal(format!("Invalid movie title: {e}"))
    //         })?,
    //         details: MediaDetailsOption::Endpoint(format!(
    //             "/movie/lookup/{}",
    //             media_file.id
    //         )),
    //         endpoint: MovieURL::from_string(format!(
    //             "/stream/{}",
    //             media_file.id
    //         )),
    //         file: media_file,
    //         theme_color: None,
    //     };

    //     self.media_refs.store_movie_reference(&movie_ref).await?;

    //     Self::annotate_context(&mut command.analyzed.context, 0);

    //     Ok(MediaReadyForIndex {
    //         library_id: command.job.library_id,
    //         logical_id: Some(movie_ref.id.to_string()),
    //         normalized_title: Some(movie_ref.title.to_string()),
    //         analyzed: command.analyzed,
    //         prepared_at: Utc::now(),
    //         image_jobs: Vec::new(),
    //     })
    // }

    async fn enrich_episode(
        &self,
        mut command: MetadataCommand,
    ) -> Result<MediaReadyForIndex> {
        let metadata =
            Self::extract_technical_metadata(&command.analyzed.analysis);
        let path = PathBuf::from(&command.analyzed.path_norm);

        let Some(info) = Self::derive_episode_info(metadata.as_ref(), &path)
        else {
            return DefaultMetadataActor::new().enrich(command).await;
        };

        let mut image_jobs = Vec::new();

        let AnalyzeScanHierarchy::Episode(episode_hierarchy) =
            command.analyzed.hierarchy.clone()
        else {
            return Err(MediaError::Internal(
                "episode enrich requires episode hierarchy".into(),
            ));
        };

        // During initial bulk seed, do not create new series from episodes.
        // If a matching series does not already exist with a real TMDB binding
        // (tmdb_id > 0), defer with a transient error so retries can resolve
        // after the series seed completes.
        let bulk_seed = matches!(command.job.scan_reason, ScanReason::BulkSeed);
        if bulk_seed {
            let locator = SeriesLocator::new(self.media_refs.clone());
            match locator.find_existing_series(&episode_hierarchy).await {
                Some(existing) if existing.tmdb_id > 0 => {}
                _ => {
                    return Err(MediaError::Internal(
                        "temporarily unavailable: series mapping not ready"
                            .into(),
                    ));
                }
            }
        }

        let season_number = info.season_number;

        let mut excluded_series = HashSet::new();
        let (series_ref, season_ref) = loop {
            let candidate_series = self
                .resolve_series(
                    command.job.library_id,
                    &info,
                    &episode_hierarchy,
                    &excluded_series,
                )
                .await?;

            match self
                .resolve_season(
                    command.job.library_id,
                    &candidate_series,
                    season_number,
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

        let library_id = command.job.library_id;

        // self.queue_image_job(
        //     command.job.library_id,
        //     "series",
        //     series_ref.id.0,
        //     MediaImageKind::Poster,
        //     0,
        //     series_ref.details.poster_path.as_deref(),
        //     true,
        //     ImageFetchPriority::Poster,
        //     &mut image_jobs,
        // )
        // .await?;
        // self.queue_image_job(
        //     command.job.library_id,
        //     "series",
        //     series_ref.id.0,
        //     MediaImageKind::Backdrop,
        //     0,
        //     series_ref.details.backdrop_path.as_deref(),
        //     true,
        //     ImageFetchPriority::Backdrop,
        //     &mut image_jobs,
        // )
        // .await?;

        // self.queue_person_profile_jobs(
        //     command.job.library_id,
        //     &series_ref.details.cast,
        //     &mut image_jobs,
        // )
        // .await?;

        // self.queue_image_job(
        //     command.job.library_id,
        //     "season",
        //     season_ref.id.0,
        //     MediaImageKind::Poster,
        //     0,
        //     season_ref.details.poster_path.as_deref(),
        //     true,
        //     ImageFetchPriority::Poster,
        //     &mut image_jobs,
        // )
        // .await?;

        let (mut episode_ref, _tmdb_episode_id) = self
            .create_episode_reference(
                &command,
                &series_ref,
                &season_ref,
                metadata.clone(),
                &info,
            )
            .await?;

        // image_jobs.push(ImageFetchJob { library_id, iid: command.job., imz: (), priority_hint: () })

        self.queue_local_episode_thumbnail(
            library_id,
            &mut episode_ref,
            &mut image_jobs,
        )
        .await?;

        let mut updated_hierarchy = episode_hierarchy.clone();
        Self::update_episode_hierarchy(
            &mut updated_hierarchy,
            &series_ref,
            &season_ref,
            &episode_ref,
            &info,
        );
        command.analyzed.hierarchy =
            AnalyzeScanHierarchy::Episode(updated_hierarchy);

        let normalized_title = Some(Self::build_episode_normalized_title(
            &series_ref,
            &season_ref,
            &episode_ref,
            &info,
        ));

        Ok(MediaReadyForIndex {
            library_id: command.job.library_id,
            media_id: MediaID::Episode(episode_ref.id),
            variant: command.analyzed.variant,
            hierarchy: command.analyzed.hierarchy.clone(),
            node: command.analyzed.node.clone(),
            normalized_title,
            analyzed: command.analyzed,
            prepared_at: Utc::now(),
            image_jobs,
        })
    }

    async fn resolve_series(
        &self,
        library_id: LibraryId,
        info: &EpisodeContextInfo,
        hierarchy: &impl crate::domain::scan::orchestration::context::WithSeriesHierarchy,
        excluded_tmdb_ids: &HashSet<u64>,
    ) -> Result<Series> {
        let locator = SeriesLocator::new(self.media_refs.clone());
        if let Some(existing) = locator.find_existing_series(hierarchy).await
            && (existing.tmdb_id == 0
                || !excluded_tmdb_ids.contains(&existing.tmdb_id))
        {
            return Ok(existing);
        }
        let (title, year, region) = if let Some(hint) = hierarchy.series_hint()
        {
            (hint.title.as_str(), hint.year.as_ref(), hint.region.clone())
        } else {
            (
                info.series.normalized_title.as_str(),
                info.series.year.as_ref(),
                info.series.region.clone(),
            )
        };

        let allow_search = !Self::is_invalid_series_title(title);

        let search_results = if allow_search {
            Some(
                self.tmdb
                    .search_series(
                        title,
                        year.copied(),
                        None,
                        region.as_deref(),
                    )
                    .await
                    .map_err(|e| {
                        MediaError::Internal(format!(
                            "TMDB series search failed: {e}"
                        ))
                    })?,
            )
        } else {
            None
        };

        let mut ordered_tmdb_ids = Vec::new();
        let mut seen_ids = HashSet::new();

        let clean_title = clean_series_title(title);

        if let Some(results) = &search_results
            && let Some(primary) = rank_series_candidates(
                &clean_title,
                year.copied(),
                &results.results,
            )
            .first()
            .map(|entry| entry.candidate)
            && primary.inner.id != 0
            && seen_ids.insert(primary.inner.id)
        {
            ordered_tmdb_ids.push(primary.inner.id);
        }

        if let Some(results) = &search_results {
            for candidate in &results.results {
                let tmdb_id = candidate.inner.id;
                if tmdb_id == 0 {
                    continue;
                }
                if seen_ids.insert(tmdb_id) {
                    ordered_tmdb_ids.push(tmdb_id);
                }
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

        Err(MediaError::InvalidMedia(format!(
            "series_not_found: {}",
            info.series.normalized_title
        )))
    }

    async fn build_series_reference(
        &self,
        library_id: LibraryId,
        tmdb_id: u64,
    ) -> Result<Series> {
        // TODO: Thread language through
        let details =
            self.tmdb.get_series(tmdb_id, None).await.map_err(|e| {
                MediaError::Internal(format!(
                    "Failed to fetch series details: {e}"
                ))
            })?;

        let credits = self.tmdb.get_series_credits(tmdb_id, None).await.ok();
        let images = self.tmdb.get_series_images(tmdb_id, None).await.ok();
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

        let mut cast = credits
            .as_ref()
            .map(Self::map_series_cast)
            .unwrap_or_default();
        let mut crew = credits
            .as_ref()
            .map(Self::map_series_crew)
            .unwrap_or_default();

        self.persist_person_profile_variants(&mut cast, &mut crew)
            .await?;

        let homepage = if details.homepage.trim().is_empty() {
            None
        } else {
            Some(details.homepage.clone())
        };

        let series_id = SeriesID::new();

        let mut primary_poster_iid: Option<Uuid> = None;
        let mut primary_backdrop_iid: Option<Uuid> = None;
        if let Some(images) = images {
            let variants = self
                .persist_tmdb_variants_series(
                    series_id.to_uuid(),
                    ImageMediaType::Series,
                    &images,
                )
                .await?;
            let inserted =
                self.image_service.images.upsert_variants(&variants).await?;

            use std::cmp::Ordering;
            let pick_primary_iid =
                |kind: fn(&ImageSize) -> bool| -> Option<Uuid> {
                    inserted
                        .iter()
                        .filter(|v| kind(&v.imz))
                        .max_by(|a, b| {
                            a.is_primary
                                .cmp(&b.is_primary)
                                .then_with(|| {
                                    a.vote_avg
                                        .partial_cmp(&b.vote_avg)
                                        .unwrap_or(Ordering::Equal)
                                })
                                .then_with(|| a.vote_cnt.cmp(&b.vote_cnt))
                        })
                        .map(|v| v.iid)
                };

            primary_poster_iid =
                pick_primary_iid(|imz| matches!(imz, ImageSize::Poster(_)));
            primary_backdrop_iid =
                pick_primary_iid(|imz| matches!(imz, ImageSize::Backdrop(_)));
        }

        if primary_poster_iid.is_none()
            && let Some(path) = details
                .inner
                .poster_path
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
        {
            let variant = self
                .image_service
                .images
                .upsert_variant(&VarInput {
                    media_id: series_id.to_uuid(),
                    media_type: ImageMediaType::Series,
                    tmdb_path: path,
                    imz: ImageSize::poster(),
                    width: 0,
                    height: 0,
                    lang: "",
                    v_avg: 0.0,
                    v_cnt: 0,
                    is_primary: true,
                })
                .await?;
            primary_poster_iid = Some(variant.iid);
        }

        if primary_backdrop_iid.is_none()
            && let Some(path) = details
                .inner
                .backdrop_path
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
        {
            let variant = self
                .image_service
                .images
                .upsert_variant(&VarInput {
                    media_id: series_id.to_uuid(),
                    media_type: ImageMediaType::Series,
                    tmdb_path: path,
                    imz: ImageSize::backdrop(),
                    width: 0,
                    height: 0,
                    lang: "",
                    v_avg: 0.0,
                    v_cnt: 0,
                    is_primary: true,
                })
                .await?;
            primary_backdrop_iid = Some(variant.iid);
        }

        let Some(primary_poster_iid) = primary_poster_iid else {
            return Err(MediaError::InvalidMedia(format!(
                "missing_primary_poster:series:{}:{}",
                tmdb_id, details.inner.name
            )));
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
            number_of_seasons: Some(details.number_of_seasons as u16),
            number_of_episodes: details.number_of_episodes.map(|n| n as u16),
            available_seasons: None,
            available_episodes: None,
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
            primary_poster_iid: Some(primary_poster_iid),
            primary_backdrop_iid,
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

        Ok(Series {
            id: series_id,
            library_id,
            tmdb_id,
            title,
            details: enhanced,
            endpoint: SeriesURL::from_string(format!("/series/{}", tmdb_id)),
            discovered_at: Utc::now(),
            created_at: Utc::now(),
            theme_color: None,
        })
    }

    async fn resolve_season(
        &self,
        library_id: LibraryId,
        series_ref: &Series,
        season_number: u16,
    ) -> Result<SeasonReference> {
        if let Some(existing) = self
            .media_refs
            .get_series_seasons(&series_ref.id)
            .await?
            .into_iter()
            .find(|season| season.season_number.value() == season_number)
        {
            return Ok(existing);
        }

        let season_details =
            // TODO: Thread language through
            match self
                .tmdb
                .get_season(series_ref.tmdb_id, season_number, None)
                .await
            {
                Ok(details) => details,
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
            };

        let mut season_ref = self
            .build_season_reference(
                library_id,
                series_ref,
                season_number,
                season_details,
            )
            .await?;

        let actual_id =
            self.media_refs.store_season_reference(&season_ref).await?;

        if MediaID::Season(season_ref.id) != actual_id {
            season_ref.id = SeasonID(actual_id.to_uuid());
        }

        Ok(season_ref)
    }

    async fn build_season_reference(
        &self,
        library_id: LibraryId,
        series_ref: &Series,
        season_number: u16,
        season_details: TmdbSeason,
    ) -> Result<SeasonReference> {
        let season_id = SeasonID::new();

        let poster_path = season_details.inner.poster_path.clone();
        let primary_poster_iid = if let Some(path) = poster_path
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            let variant = self
                .image_service
                .images
                .upsert_variant(&VarInput {
                    media_id: season_id.to_uuid(),
                    media_type: ImageMediaType::Season,
                    tmdb_path: path,
                    imz: ImageSize::poster(),
                    width: 0,
                    height: 0,
                    lang: "",
                    v_avg: 0.0,
                    v_cnt: 0,
                    is_primary: true,
                })
                .await?;
            variant.iid
        } else {
            return Err(MediaError::InvalidMedia(format!(
                "missing_primary_poster_path:season:{}:{}:{}",
                series_ref.tmdb_id, season_details.inner.id, season_number
            )));
        };

        let details = {
            let name = if season_details.inner.name.trim().is_empty() {
                format!("Season {}", season_number)
            } else {
                season_details.inner.name.clone()
            };

            SeasonDetails {
                id: season_details.inner.id,
                season_number: season_details.inner.season_number as u16,
                name,
                overview: season_details.inner.overview.clone(),
                air_date: season_details
                    .inner
                    .air_date
                    .as_ref()
                    .map(|d| d.to_string()),
                episode_count: season_details.episodes.len() as u16,
                poster_path,
                primary_poster_iid: Some(primary_poster_iid),
                runtime: None,
                external_ids: ExternalIds::default(),
                images: MediaImages::default(),
                videos: Vec::new(),
                keywords: Vec::new(),
                translations: Vec::new(),
            }
        };

        let endpoint_path = if series_ref.tmdb_id > 0 {
            format!("/series/{}/season/{}", series_ref.tmdb_id, season_number)
        } else {
            format!("/series/{}/season/{}", series_ref.id, season_number)
        };

        let endpoint = SeasonURL::from_string(endpoint_path.clone());

        Ok(SeasonReference {
            id: season_id,
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
        series_ref: &Series,
        season_ref: &SeasonReference,
        metadata: Option<MediaFileMetadata>,
        info: &EpisodeContextInfo,
    ) -> Result<(EpisodeReference, u64)> {
        let allow_zero_length = {
            #[cfg(feature = "demo")]
            {
                crate::domain::demo::allow_zero_length_for(
                    &command.analyzed.library_id,
                )
            }
            #[cfg(not(feature = "demo"))]
            {
                false
            }
        };

        let episode_id = EpisodeID::new();

        let mut media_file = MediaFile::new_with_policy(
            MediaID::Episode(episode_id),
            PathBuf::from(&command.analyzed.path_norm),
            command.analyzed.library_id,
            allow_zero_length,
        )?;

        if let Some(meta) = metadata.clone() {
            media_file.media_file_metadata = Some(meta);
        }

        let upsert = self.media_files_write.upsert(media_file.clone()).await?;
        let actual_file_id = upsert.id;
        media_file.id = actual_file_id;

        // let (episode_details, tmdb_episode_id) = if series_ref.tmdb_id > 0 {
        let (mut details, tmdb_episode_id) = {
            // TODO: Thread language through
            match self
                .tmdb
                .get_episode(
                    series_ref.tmdb_id,
                    season_ref.season_number.value(),
                    info.episode_number,
                    None,
                )
                .await
            {
                Ok(details) => {
                    let mapped = EpisodeDetails {
                        id: details.inner.id,
                        episode_number: details.inner.episode_number as u16,
                        season_number: details.inner.season_number as u16,
                        name: details.inner.name.clone(),
                        overview: details.inner.overview.clone(),
                        air_date: details
                            .inner
                            .air_date
                            .as_ref()
                            .map(|d| d.to_string()),
                        runtime: None,
                        still_path: details.inner.still_path.clone(),
                        primary_still_iid: None,
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
                    (mapped, details.inner.id)
                }
                Err(ProviderError::ApiError(msg)) if msg.contains("404") => {
                    return Err(MediaError::InvalidMedia(format!(
                        "{}:{}:{}:{}",
                        EPISODE_NOT_FOUND_PREFIX,
                        series_ref.tmdb_id,
                        season_ref.season_number.value(),
                        info.episode_number
                    )));
                }
                Err(err) => {
                    return Err(MediaError::Internal(format!(
                        "Failed to fetch episode details for series {} S{}E{}: {err}",
                        series_ref.tmdb_id,
                        season_ref.season_number.value(),
                        info.episode_number
                    )));
                }
            }
        };

        // TODO: Pass width and height for thumbnail
        if let Some(path) = details
            .still_path
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            let variant = self
                .image_service
                .images
                .upsert_variant(&VarInput {
                    media_id: episode_id.to_uuid(),
                    media_type: ImageMediaType::Episode,
                    tmdb_path: path,
                    imz: ImageSize::thumbnail(),
                    width: 0,
                    height: 0,
                    lang: "",
                    v_avg: 0.0,
                    v_cnt: 0,
                    is_primary: true,
                })
                .await?;
            details.primary_still_iid = Some(variant.iid);
        }

        let file_discovered_at = media_file.discovered_at;
        let file_created_at = media_file.created_at;

        let episode_ref = EpisodeReference {
            id: episode_id,
            library_id: command.analyzed.library_id,
            episode_number: EpisodeNumber::new(info.episode_number),
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

    async fn persist_person_profile_variants(
        &self,
        cast: &mut [CastMember],
        crew: &mut [CrewMember],
    ) -> Result<()> {
        use ferrex_model::ProfileSize;

        for member in cast.iter_mut() {
            if let (Some(path), Some(person_id)) = (
                member
                    .profile_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|p| !p.is_empty()),
                member.person_id,
            ) {
                let variant = self
                    .image_service
                    .images
                    .upsert_variant(&VarInput {
                        media_id: person_id,
                        media_type: ImageMediaType::Person,
                        tmdb_path: path,
                        imz: ImageSize::Profile(ProfileSize::W185),
                        width: 0,
                        height: 0,
                        lang: "",
                        v_avg: 0.0,
                        v_cnt: 0,
                        is_primary: true,
                    })
                    .await?;
                member.image_id = Some(variant.iid);
            } else {
                continue;
            }
        }

        for member in crew.iter_mut() {
            if let (Some(path), Some(person_id)) = (
                member
                    .profile_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|p| !p.is_empty()),
                member.person_id,
            ) {
                let variant = self
                    .image_service
                    .images
                    .upsert_variant(&VarInput {
                        media_id: person_id,
                        media_type: ImageMediaType::Person,
                        tmdb_path: path,
                        imz: ImageSize::Profile(ProfileSize::W185),
                        width: 0,
                        height: 0,
                        lang: "",
                        v_avg: 0.0,
                        v_cnt: 0,
                        is_primary: true,
                    })
                    .await?;
                member.profile_iid = Some(variant.iid);
            } else {
                continue;
            }
        }

        Ok(())
    }

    fn update_episode_hierarchy(
        hierarchy: &mut crate::domain::scan::orchestration::context::EpisodeScanHierarchy,
        series_ref: &Series,
        season_ref: &SeasonReference,
        episode_ref: &EpisodeReference,
        info: &EpisodeContextInfo,
    ) {
        hierarchy.series = SeriesLink::Resolved(SeriesRef {
            id: series_ref.id,
            slug: None,
            title: Some(series_ref.title.as_ref().to_string()),
        });
        hierarchy.season = SeasonLink::Resolved(SeasonRef {
            id: season_ref.id,
            number: Some(season_ref.season_number.value()),
        });
        hierarchy.episode = EpisodeLink::Resolved(EpisodeRef {
            id: episode_ref.id,
            number: Some(episode_ref.episode_number.value()),
            title: info.episode_title.clone(),
        });
    }

    fn build_episode_normalized_title(
        series_ref: &Series,
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
}

#[async_trait]
impl MetadataActor for TmdbMetadataActor {
    async fn enrich(
        &self,
        command: MetadataCommand,
    ) -> Result<MediaReadyForIndex> {
        match command.job.variant {
            VideoMediaType::Series => self.enrich_series(command).await,
            VideoMediaType::Movie => self.enrich_movie(command).await,
            VideoMediaType::Season => {
                DefaultMetadataActor::new().enrich(command).await
            }
            VideoMediaType::Episode => self.enrich_episode(command).await,
        }
    }
}

#[async_trait]
impl SeriesMetadataProvider for TmdbMetadataActor {
    async fn resolve_series(
        &self,
        library_id: LibraryId,
        series_root_path: &SeriesRootPath,
        hint: &SeriesHint,
        _folder_name: &str,
    ) -> Result<SeriesResolution> {
        let series_ref =
            self.resolve_series_from_hint(library_id, hint).await?;
        let hierarchy = SeriesScanHierarchy::new(
            SeriesLink::Resolved(SeriesRef {
                id: series_ref.id,
                slug: hint.slug.clone(),
                title: Some(series_ref.title.as_str().to_string()),
            }),
            series_root_path.clone(),
        );

        let analyzed = MediaAnalyzed {
            library_id,
            media_id: MediaID::Series(series_ref.id),
            variant: VideoMediaType::Series,
            hierarchy: AnalyzeScanHierarchy::Series(hierarchy.clone()),
            node: ScanNodeKind::SeriesRoot,
            path_norm: series_root_path.as_str().to_string(),
            fingerprint: MediaFingerprint::default(),
            analyzed_at: Utc::now(),
            analysis: crate::domain::scan::actors::analyze::AnalysisContext {
                technical: None,
                demo_note: None,
                tmdb_id_hint: Some(series_ref.tmdb_id),
            },
            thumbnails: Vec::new(),
        };

        let ready = MediaReadyForIndex {
            library_id,
            media_id: analyzed.media_id,
            variant: analyzed.variant,
            hierarchy: AnalyzeScanHierarchy::Series(hierarchy.clone()),
            node: analyzed.node.clone(),
            normalized_title: Some(series_ref.title.as_str().to_string()),
            analyzed,
            prepared_at: Utc::now(),
            image_jobs: vec![],
        };

        Ok(SeriesResolution {
            series_ref: SeriesRef {
                id: series_ref.id,
                slug: hint.slug.clone(),
                title: Some(series_ref.title.as_str().to_string()),
            },
            ready,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tmdb_api::tvshow::content_rating::ContentRating as TmdbContentRating;

    #[test]
    fn movie_primary_image_jobs_skips_backdrop_when_missing() {
        let library_id = LibraryId::new();
        let poster_iid = Uuid::new_v4();

        let jobs = TmdbMetadataActor::movie_primary_image_jobs(
            library_id, poster_iid, None,
        );

        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].library_id, library_id);
        assert_eq!(jobs[0].iid, poster_iid);
        assert_eq!(jobs[0].imz, ImageSize::poster());
    }

    #[test]
    fn movie_primary_image_jobs_includes_backdrop_when_present() {
        let library_id = LibraryId::new();
        let poster_iid = Uuid::new_v4();
        let backdrop_iid = Uuid::new_v4();

        let jobs = TmdbMetadataActor::movie_primary_image_jobs(
            library_id,
            poster_iid,
            Some(backdrop_iid),
        );

        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].iid, poster_iid);
        assert_eq!(jobs[0].imz, ImageSize::poster());
        assert_eq!(jobs[1].iid, backdrop_iid);
        assert_eq!(jobs[1].imz, ImageSize::backdrop());
    }

    #[test]
    fn parse_movie_folder_name_extracts_title_and_year() {
        let (title, year) =
            TmdbMetadataActor::parse_movie_folder_name("Alien (1979)");
        assert_eq!(title, "Alien");
        assert_eq!(year, Some(1979));
    }

    #[test]
    fn parse_movie_folder_name_passes_through_when_year_missing() {
        let (title, year) = TmdbMetadataActor::parse_movie_folder_name("Alien");
        assert_eq!(title, "Alien");
        assert_eq!(year, None);
    }

    #[test]
    fn map_series_content_ratings_normalizes_and_dedupes() {
        let data = SeriesContentRatingResponse {
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
