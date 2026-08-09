use std::{
    collections::HashSet, net::SocketAddr, path::PathBuf, time::Duration,
};

use anyhow::{Context, Result};
use axum::{Router, body::Bytes, http::StatusCode, http::header};
use axum_test::{TestResponse, TestServer};
use chrono::Utc;
use ferrex_core::{
    api::routes::{utils::replace_param, v1},
    domain::scan::{
        AnalyzeScanHierarchy,
        actors::{
            FolderScanOutcome, FolderScanSummary, MediaFileDiscovered,
            MediaKindHint,
            index::{IndexingChange, IndexingOutcome},
        },
        orchestration::{
            ScanReason,
            context::{
                EpisodeHint, EpisodeLink, EpisodeRef, EpisodeScanHierarchy,
                FolderScanContext, ScanNodeKind, SeasonFolderPath,
                SeasonFolderScanContext, SeasonLink, SeasonRef,
                SeasonScanHierarchy, SeriesFolderScanContext, SeriesHint,
                SeriesLink, SeriesRef, SeriesRootPath, SeriesScanHierarchy,
            },
            events::{ScanEvent, ScanEventPublisher},
            job::MediaFingerprint,
        },
    },
    infra::cache::{
        ImageBlobStore, ImageCacheRoot, ImageFileStore, image_cache_key_for,
    },
};
use ferrex_flatbuffers::{
    FLATBUFFERS_MIME,
    conversions::batch_sync,
    fb,
    uuid_helpers::{fb_to_uuid, uuid_to_fb},
};
use ferrex_model::library::LibraryLikeMut;
use ferrex_model::{
    EpisodeID, EpisodeReference, ImageSize, Library, LibraryId, LibraryType,
    MediaEvent, MediaFile, MediaFileMetadata, MediaID, MovieID, MovieReference,
    SeasonID, SeasonReference, Series, SeriesID, VideoMediaType,
};
use flatbuffers::FlatBufferBuilder;
use rkyv::util::AlignedVec;
use serde_json::{Value, json};
use sqlx::PgPool;
use tempfile::TempDir;
use uuid::Uuid;

use ferrex_server::infra::{app_state::AppState, startup::NoopStartupHooks};

mod common;
use common::build_test_app_with_hooks;

const LARGE_SERIES_COUNT: usize = 80;

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

async fn build_server(pool: PgPool) -> Result<(TestServer, AppState, TempDir)> {
    let app = build_test_app_with_hooks(pool, &NoopStartupHooks).await?;
    let (router, state, tempdir) = app.into_parts();
    let router: Router<()> = router.with_state(state.clone());
    let make_service =
        router.into_make_service_with_connect_info::<SocketAddr>();
    let server = TestServer::builder()
        .http_transport()
        .build(make_service)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    Ok((server, state, tempdir))
}

async fn login_test_user(
    server: &TestServer,
    username: &str,
) -> Result<String> {
    let password = "Password#123";
    let register = server
        .post(v1::auth::REGISTER)
        .json(&json!({
            "username": username,
            "display_name": username,
            "password": password,
        }))
        .await;
    register.assert_status_ok();

    let login = server
        .post(v1::auth::LOGIN)
        .json(&json!({
            "username": username,
            "password": password,
        }))
        .await;
    login.assert_status_ok();
    let body: Value = login.json();
    body["data"]["access_token"]
        .as_str()
        .map(ToOwned::to_owned)
        .context("login response missing access_token")
}

fn content_type(response: &TestResponse) -> Result<String> {
    response
        .maybe_header(header::CONTENT_TYPE)
        .context("Content-Type header missing")?
        .to_str()
        .context("Content-Type header must be UTF-8")
        .map(ToOwned::to_owned)
}

fn assert_content_type_starts_with(
    response: &TestResponse,
    expected: &str,
) -> Result<()> {
    let content_type = content_type(response)?;
    assert!(
        content_type.starts_with(expected),
        "expected Content-Type {expected}, got {content_type}"
    );
    Ok(())
}

fn aligned(bytes: &[u8]) -> AlignedVec {
    let mut out = AlignedVec::with_capacity(bytes.len());
    out.extend_from_slice(bytes);
    out
}

fn make_library(name: &str, library_type: LibraryType) -> Library {
    Library::new(
        name.to_string(),
        library_type,
        vec![PathBuf::from(format!("/test/{name}"))],
    )
}

async fn create_library(
    state: &AppState,
    library: Library,
) -> Result<LibraryId> {
    state
        .unit_of_work()
        .libraries
        .create_library(library)
        .await
        .context("create library")
}

fn make_movie_details(title: &str) -> ferrex_model::EnhancedMovieDetails {
    ferrex_model::EnhancedMovieDetails {
        id: 100,
        title: title.to_string(),
        original_title: Some(title.to_string()),
        overview: Some("A test movie".to_string()),
        release_date: Some("2026-01-02".to_string()),
        runtime: Some(123),
        vote_average: Some(8.5),
        vote_count: Some(42),
        popularity: Some(99.0),
        content_rating: Some("PG-13".to_string()),
        content_ratings: Vec::new(),
        release_dates: Vec::new(),
        genres: vec![ferrex_model::GenreInfo {
            id: 1,
            name: "Adventure".to_string(),
        }],
        spoken_languages: Vec::new(),
        production_companies: Vec::new(),
        production_countries: Vec::new(),
        homepage: None,
        status: Some("Released".to_string()),
        tagline: None,
        budget: None,
        revenue: None,
        poster_path: None,
        backdrop_path: None,
        logo_path: None,
        primary_poster_iid: None,
        primary_backdrop_iid: None,
        images: ferrex_model::image::MediaImages::default(),
        cast: Vec::new(),
        crew: Vec::new(),
        videos: Vec::new(),
        keywords: Vec::new(),
        external_ids: ferrex_model::details::ExternalIds::default(),
        alternative_titles: Vec::new(),
        translations: Vec::new(),
        collection: None,
        recommendations: Vec::new(),
        similar: Vec::new(),
    }
}

fn make_series_details(name: &str) -> ferrex_model::EnhancedSeriesDetails {
    ferrex_model::EnhancedSeriesDetails {
        id: 200,
        name: name.to_string(),
        original_name: Some(name.to_string()),
        overview: Some("A test series".to_string()),
        first_air_date: Some("2026-02-03".to_string()),
        last_air_date: None,
        number_of_seasons: Some(1),
        number_of_episodes: Some(1),
        available_seasons: Some(1),
        available_episodes: Some(1),
        vote_average: Some(7.5),
        vote_count: Some(24),
        popularity: Some(88.0),
        content_rating: Some("TV-14".to_string()),
        content_ratings: Vec::new(),
        release_dates: Vec::new(),
        genres: vec![ferrex_model::GenreInfo {
            id: 2,
            name: "Drama".to_string(),
        }],
        networks: Vec::new(),
        origin_countries: Vec::new(),
        spoken_languages: Vec::new(),
        production_companies: Vec::new(),
        production_countries: Vec::new(),
        homepage: None,
        status: Some("Returning Series".to_string()),
        tagline: None,
        in_production: Some(true),
        poster_path: None,
        backdrop_path: None,
        logo_path: None,
        primary_poster_iid: None,
        primary_backdrop_iid: None,
        images: ferrex_model::image::MediaImages::default(),
        cast: Vec::new(),
        crew: Vec::new(),
        videos: Vec::new(),
        keywords: Vec::new(),
        external_ids: ferrex_model::details::ExternalIds::default(),
        alternative_titles: Vec::new(),
        translations: Vec::new(),
        episode_groups: Vec::new(),
        recommendations: Vec::new(),
        similar: Vec::new(),
    }
}

fn make_season_details() -> ferrex_model::SeasonDetails {
    ferrex_model::SeasonDetails {
        id: 300,
        season_number: 1,
        name: "Season 1".to_string(),
        overview: Some("Opening season".to_string()),
        air_date: Some("2026-02-03".to_string()),
        episode_count: 1,
        poster_path: None,
        primary_poster_iid: None,
        runtime: Some(50),
        external_ids: ferrex_model::details::ExternalIds::default(),
        images: ferrex_model::image::MediaImages::default(),
        videos: Vec::new(),
        keywords: Vec::new(),
        translations: Vec::new(),
    }
}

fn make_episode_details() -> ferrex_model::EpisodeDetails {
    ferrex_model::EpisodeDetails {
        id: 400,
        episode_number: 1,
        season_number: 1,
        name: "Pilot".to_string(),
        overview: Some("The first episode".to_string()),
        air_date: Some("2026-02-03".to_string()),
        runtime: Some(50),
        still_path: None,
        primary_still_iid: None,
        vote_average: Some(8.0),
        vote_count: Some(10),
        production_code: Some("S01E01".to_string()),
        external_ids: ferrex_model::details::ExternalIds::default(),
        images: ferrex_model::image::MediaImages::default(),
        videos: Vec::new(),
        keywords: Vec::new(),
        translations: Vec::new(),
        guest_stars: Vec::new(),
        crew: Vec::new(),
        content_ratings: Vec::new(),
    }
}

fn make_media_file(media_id: MediaID, library_id: LibraryId) -> MediaFile {
    make_media_file_at_path(
        media_id,
        library_id,
        PathBuf::from(format!("/media/{}.mkv", Uuid::now_v7())),
    )
}

fn make_media_file_at_path(
    media_id: MediaID,
    library_id: LibraryId,
    path: PathBuf,
) -> MediaFile {
    let now = Utc::now();
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file.mkv")
        .to_string();
    MediaFile {
        id: Uuid::now_v7(),
        media_id,
        path,
        filename,
        size: 123_456,
        discovered_at: now,
        created_at: now,
        media_file_metadata: Some(MediaFileMetadata {
            duration: Some(3600.0),
            width: Some(1920),
            height: Some(1080),
            video_codec: Some("h264".to_string()),
            audio_codec: Some("aac".to_string()),
            bitrate: Some(8_000_000),
            framerate: Some(23.976),
            file_size: 123_456,
            color_primaries: None,
            color_transfer: None,
            color_space: None,
            bit_depth: Some(8),
            parsed_info: None,
        }),
        library_id,
    }
}

async fn seed_primary_image(
    pool: &PgPool,
    media_id: Uuid,
    media_type: &str,
    image_variant: &str,
    iid: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO tmdb_image_variants (
            id,
            image_variant,
            tmdb_path,
            media_id,
            media_type,
            width,
            height,
            vote_avg,
            vote_cnt,
            is_primary
        )
        VALUES ($1, $2::image_variant, $3, $4, $5::media_type, 1, 1, 0.0, 0, true)
        "#,
    )
    .bind(iid)
    .bind(image_variant)
    .bind(format!("t_{}", iid.simple()))
    .bind(media_id)
    .bind(media_type)
    .execute(pool)
    .await
    .context("insert tmdb image variant")?;

    Ok(())
}

async fn seed_movie(
    state: &AppState,
    pool: &PgPool,
) -> Result<(LibraryId, MovieID, u32)> {
    let library_id =
        create_library(state, make_library("Movies", LibraryType::Movies))
            .await?;
    let movie_id = MovieID(Uuid::now_v7());
    let poster_iid = Uuid::now_v7();
    seed_primary_image(pool, movie_id.to_uuid(), "movie", "poster", poster_iid)
        .await?;
    let mut details = make_movie_details("Test Movie");
    details.primary_poster_iid = Some(poster_iid);
    let movie = MovieReference {
        id: movie_id,
        library_id,
        batch_id: None,
        tmdb_id: 100,
        title: "Test Movie".into(),
        details,
        endpoint: "/api/v1/stream/movie".to_string().into(),
        file: make_media_file(MediaID::Movie(movie_id), library_id),
        theme_color: Some("#112233".to_string()),
    };

    state
        .unit_of_work()
        .media_refs
        .store_movie_reference(&movie)
        .await
        .context("store movie")?;
    let stored = state
        .unit_of_work()
        .media_refs
        .get_movie_reference(&movie_id)
        .await
        .context("fetch stored movie")?;
    let batch_id = stored
        .batch_id
        .context("stored movie should have batch id")?
        .as_u32();

    Ok((library_id, movie_id, batch_id))
}

async fn seed_series(
    state: &AppState,
    pool: &PgPool,
) -> Result<(LibraryId, SeriesID, SeasonID, EpisodeID)> {
    let library_id =
        create_library(state, make_library("Series", LibraryType::Series))
            .await?;
    let series_id = SeriesID(Uuid::now_v7());
    let season_id = SeasonID(Uuid::now_v7());
    let episode_id = EpisodeID(Uuid::now_v7());
    let series_poster_iid = Uuid::now_v7();
    let season_poster_iid = Uuid::now_v7();
    seed_primary_image(
        pool,
        series_id.to_uuid(),
        "series",
        "poster",
        series_poster_iid,
    )
    .await?;
    seed_primary_image(
        pool,
        season_id.to_uuid(),
        "season",
        "poster",
        season_poster_iid,
    )
    .await?;
    let now = Utc::now();

    let mut series_details = make_series_details("Test Series");
    series_details.primary_poster_iid = Some(series_poster_iid);
    let series = Series {
        id: series_id,
        library_id,
        tmdb_id: 200,
        title: "Test Series".into(),
        details: series_details,
        endpoint: "/api/v1/series/test".to_string().into(),
        discovered_at: now,
        created_at: now,
        theme_color: Some("#334455".to_string()),
    };
    let mut season_details = make_season_details();
    season_details.primary_poster_iid = Some(season_poster_iid);
    let season = SeasonReference {
        id: season_id,
        library_id,
        season_number: 1.into(),
        series_id,
        tmdb_series_id: 200,
        details: season_details,
        endpoint: "/api/v1/series/test/season/1".to_string().into(),
        discovered_at: now,
        created_at: now,
        theme_color: Some("#334455".to_string()),
    };
    let episode = EpisodeReference {
        id: episode_id,
        library_id,
        episode_number: 1.into(),
        season_number: 1.into(),
        season_id,
        series_id,
        tmdb_series_id: 200,
        details: make_episode_details(),
        endpoint: "/api/v1/series/test/season/1/episode/1".to_string().into(),
        file: make_media_file(MediaID::Episode(episode_id), library_id),
        discovered_at: now,
        created_at: now,
    };

    state
        .unit_of_work()
        .media_refs
        .store_series_reference(&series)
        .await
        .context("store series")?;
    state
        .unit_of_work()
        .media_refs
        .store_season_reference(&season)
        .await
        .context("store season")?;
    state
        .unit_of_work()
        .media_refs
        .store_episode_reference(&episode)
        .await
        .context("store episode")?;

    Ok((library_id, series_id, season_id, episode_id))
}

async fn seed_series_in_library(
    state: &AppState,
    pool: &PgPool,
    library_id: LibraryId,
    index: usize,
) -> Result<SeriesID> {
    let series_id = SeriesID(Uuid::now_v7());
    let season_id = SeasonID(Uuid::now_v7());
    let episode_id = EpisodeID(Uuid::now_v7());
    let tmdb_id = 20_000 + index as u64;
    let title = format!("Large Fixture Series {index:03}");
    let slug = format!("large-fixture-series-{index:03}");
    let series_poster_iid = Uuid::now_v7();
    let season_poster_iid = Uuid::now_v7();
    seed_primary_image(
        pool,
        series_id.to_uuid(),
        "series",
        "poster",
        series_poster_iid,
    )
    .await?;
    seed_primary_image(
        pool,
        season_id.to_uuid(),
        "season",
        "poster",
        season_poster_iid,
    )
    .await?;
    let now = Utc::now();

    let mut series_details = make_series_details(&title);
    series_details.id = tmdb_id;
    series_details.name = title.clone();
    series_details.original_name = Some(title.clone());
    series_details.number_of_episodes = Some(1);
    series_details.available_episodes = Some(1);
    series_details.primary_poster_iid = Some(series_poster_iid);

    let series = Series {
        id: series_id,
        library_id,
        tmdb_id,
        title: title.clone().into(),
        details: series_details,
        endpoint: format!("/api/v1/series/{slug}").into(),
        discovered_at: now,
        created_at: now,
        theme_color: None,
    };

    let mut season_details = make_season_details();
    season_details.id = 30_000 + index as u64;
    season_details.name = format!("Season {index:03}");
    season_details.primary_poster_iid = Some(season_poster_iid);
    let season = SeasonReference {
        id: season_id,
        library_id,
        season_number: 1.into(),
        series_id,
        tmdb_series_id: tmdb_id,
        details: season_details,
        endpoint: format!("/api/v1/series/{slug}/season/1").into(),
        discovered_at: now,
        created_at: now,
        theme_color: None,
    };

    let mut episode_details = make_episode_details();
    episode_details.id = 40_000 + index as u64;
    episode_details.name = format!("Episode {index:03}");
    episode_details.production_code = Some(format!("LFS{index:03}"));
    let episode = EpisodeReference {
        id: episode_id,
        library_id,
        episode_number: 1.into(),
        season_number: 1.into(),
        season_id,
        series_id,
        tmdb_series_id: tmdb_id,
        details: episode_details,
        endpoint: format!("/api/v1/series/{slug}/season/1/episode/1").into(),
        file: make_media_file(MediaID::Episode(episode_id), library_id),
        discovered_at: now,
        created_at: now,
    };

    state
        .unit_of_work()
        .media_refs
        .store_series_reference(&series)
        .await
        .context("store large fixture series")?;
    state
        .unit_of_work()
        .media_refs
        .store_season_reference(&season)
        .await
        .context("store large fixture season")?;
    state
        .unit_of_work()
        .media_refs
        .store_episode_reference(&episode)
        .await
        .context("store large fixture episode")?;

    Ok(series_id)
}

async fn seed_large_series_library(
    state: &AppState,
    pool: &PgPool,
    count: usize,
) -> Result<(LibraryId, Vec<SeriesID>)> {
    let library_id = create_library(
        state,
        make_library("Large Series", LibraryType::Series),
    )
    .await?;
    let mut series_ids = Vec::with_capacity(count);
    for index in 0..count {
        series_ids.push(
            seed_series_in_library(state, pool, library_id, index).await?,
        );
    }
    series_ids.sort_by_key(|id| id.to_uuid());
    Ok((library_id, series_ids))
}

fn fb_image_manifest_request(iid: Uuid) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();
    let iid = uuid_to_fb(&iid);
    let query = fb::image::ImageQuery::create(
        &mut builder,
        &fb::image::ImageQueryArgs {
            iid: Some(&iid),
            category: fb::common::ImageCategory::Poster,
        },
    );
    let queries = builder.create_vector(&[query]);
    let request = fb::image::ImageManifestRequest::create(
        &mut builder,
        &fb::image::ImageManifestRequestArgs {
            queries: Some(queries),
        },
    );
    builder.finish(request, None);
    builder.finished_data().to_vec()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedSeriesBundleSync {
    stale_series_ids: Vec<Uuid>,
    deleted_series_ids: Vec<Uuid>,
    server_versions: Vec<batch_sync::SeriesBundleVersion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedSeriesBundleFetch {
    series_ids: Vec<Uuid>,
    versions: Vec<u64>,
    item_counts: Vec<usize>,
    item_variants: Vec<Vec<fb::media::MediaVariant>>,
}

fn sorted_series_uuids(series_ids: &[SeriesID]) -> Vec<Uuid> {
    let mut ids = series_ids.iter().map(|id| id.to_uuid()).collect::<Vec<_>>();
    ids.sort_unstable();
    ids
}

fn sorted_unique_uuids(mut ids: Vec<Uuid>) -> Vec<Uuid> {
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn series_version_ids(
    versions: &[batch_sync::SeriesBundleVersion],
) -> Vec<Uuid> {
    versions.iter().map(|version| version.series_id).collect()
}

fn assert_unique_uuids(ids: &[Uuid]) {
    let unique = ids.iter().copied().collect::<HashSet<_>>();
    assert_eq!(unique.len(), ids.len(), "duplicate UUIDs: {ids:?}");
}

async fn versioning_row_count(
    pool: &PgPool,
    library_id: LibraryId,
) -> Result<i64> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM series_bundle_versioning WHERE library_id = $1",
    )
    .bind(library_id.to_uuid())
    .fetch_one(pool)
    .await
    .context("count series bundle versioning rows")
}

async fn series_bundle_versions(
    state: &AppState,
    library_id: LibraryId,
) -> Result<Vec<batch_sync::SeriesBundleVersion>> {
    let mut versions = state
        .unit_of_work()
        .media_refs
        .list_finalized_series_bundle_versions(&library_id)
        .await
        .context("list finalized series bundle versions")?;
    versions.sort_by_key(|record| record.series_id.to_uuid());
    Ok(versions
        .into_iter()
        .map(|record| batch_sync::SeriesBundleVersion {
            series_id: record.series_id.to_uuid(),
            version: record.version,
        })
        .collect())
}

fn parse_series_bundle_sync_response(
    bytes: &[u8],
) -> Result<ParsedSeriesBundleSync> {
    let response =
        flatbuffers::root::<fb::library::SeriesBundleSyncResponse>(bytes)?;

    let stale = response
        .stale_series_ids()
        .context("missing stale_series_ids")?;
    let mut stale_series_ids = Vec::with_capacity(stale.len());
    for index in 0..stale.len() {
        let id = stale.get(index);
        stale_series_ids.push(fb_to_uuid(&id));
    }

    let deleted = response
        .deleted_series_ids()
        .context("missing deleted_series_ids")?;
    let mut deleted_series_ids = Vec::with_capacity(deleted.len());
    for index in 0..deleted.len() {
        let id = deleted.get(index);
        deleted_series_ids.push(fb_to_uuid(&id));
    }

    let versions = response
        .server_versions()
        .context("missing server_versions")?;
    let mut server_versions = Vec::with_capacity(versions.len());
    for index in 0..versions.len() {
        let version = versions.get(index);
        server_versions.push(batch_sync::SeriesBundleVersion {
            series_id: fb_to_uuid(version.series_id()),
            version: version.version(),
        });
    }

    Ok(ParsedSeriesBundleSync {
        stale_series_ids,
        deleted_series_ids,
        server_versions,
    })
}

fn parse_series_bundle_fetch_response(
    bytes: &[u8],
) -> Result<ParsedSeriesBundleFetch> {
    let response =
        flatbuffers::root::<fb::library::SeriesBundleFetchResponse>(bytes)?;
    let bundles = response.bundles().context("missing bundles")?;
    let mut series_ids = Vec::with_capacity(bundles.len());
    let mut versions = Vec::with_capacity(bundles.len());
    let mut item_counts = Vec::with_capacity(bundles.len());
    let mut item_variants = Vec::with_capacity(bundles.len());
    for index in 0..bundles.len() {
        let bundle = bundles.get(index);
        series_ids.push(fb_to_uuid(bundle.series_id()));
        versions.push(bundle.version());
        let items = bundle.items().context("missing bundle items")?;
        item_counts.push(items.len());
        let mut variants = Vec::with_capacity(items.len());
        for item_index in 0..items.len() {
            variants.push(items.get(item_index).variant_type());
        }
        item_variants.push(variants);
    }

    Ok(ParsedSeriesBundleFetch {
        series_ids,
        versions,
        item_counts,
        item_variants,
    })
}

async fn post_series_bundle_sync_flatbuffers(
    server: &TestServer,
    token: &str,
    library_id: LibraryId,
    manifest: &[batch_sync::SeriesBundleVersion],
) -> Result<ParsedSeriesBundleSync> {
    let sync_path = replace_param(
        v1::libraries::series_bundles::SYNC,
        "{id}",
        library_id.to_uuid().to_string(),
    );
    let response = server
        .post(&sync_path)
        .add_header("Authorization", bearer(token))
        .add_header("Accept", FLATBUFFERS_MIME)
        .content_type(FLATBUFFERS_MIME)
        .bytes(Bytes::from(
            batch_sync::serialize_series_bundle_sync_request(manifest),
        ))
        .await;
    response.assert_status_ok();
    assert_content_type_starts_with(&response, FLATBUFFERS_MIME)?;
    parse_series_bundle_sync_response(response.as_bytes().as_ref())
}

async fn seed_series_records(
    state: &AppState,
    pool: &PgPool,
    library_id: LibraryId,
    series_id: SeriesID,
    season_id: SeasonID,
    episode_id: EpisodeID,
    episode_path: &str,
) -> Result<()> {
    let now = Utc::now();
    let title = "Finalized Contract Series";
    let series_poster_iid = Uuid::now_v7();
    let season_poster_iid = Uuid::now_v7();
    seed_primary_image(
        pool,
        series_id.to_uuid(),
        "series",
        "poster",
        series_poster_iid,
    )
    .await?;
    seed_primary_image(
        pool,
        season_id.to_uuid(),
        "season",
        "poster",
        season_poster_iid,
    )
    .await?;
    let mut series_details = make_series_details(title);
    series_details.id = 9_200;
    series_details.primary_poster_iid = Some(series_poster_iid);
    let mut season_details = make_season_details();
    season_details.id = 9_201;
    season_details.primary_poster_iid = Some(season_poster_iid);
    let mut episode_details = make_episode_details();
    episode_details.id = 9_202;
    let series = Series {
        id: series_id,
        library_id,
        tmdb_id: 9_200,
        title: title.into(),
        details: series_details,
        endpoint: "/api/v1/series/finalized-contract".to_string().into(),
        discovered_at: now,
        created_at: now,
        theme_color: None,
    };
    let season = SeasonReference {
        id: season_id,
        library_id,
        season_number: 1.into(),
        series_id,
        tmdb_series_id: 9_200,
        details: season_details,
        endpoint: "/api/v1/series/finalized-contract/season/1"
            .to_string()
            .into(),
        discovered_at: now,
        created_at: now,
        theme_color: None,
    };
    let episode = EpisodeReference {
        id: episode_id,
        library_id,
        episode_number: 1.into(),
        season_number: 1.into(),
        season_id,
        series_id,
        tmdb_series_id: 9_200,
        details: episode_details,
        endpoint: "/api/v1/series/finalized-contract/season/1/episode/1"
            .to_string()
            .into(),
        file: make_media_file_at_path(
            MediaID::Episode(episode_id),
            library_id,
            PathBuf::from(episode_path),
        ),
        discovered_at: now,
        created_at: now,
    };

    state
        .unit_of_work()
        .media_refs
        .store_series_reference(&series)
        .await
        .context("store finalization series")?;
    state
        .unit_of_work()
        .media_refs
        .store_season_reference(&season)
        .await
        .context("store finalization season")?;
    state
        .unit_of_work()
        .media_refs
        .store_episode_reference(&episode)
        .await
        .context("store finalization episode")?;

    Ok(())
}

async fn series_bundle_version_row(
    pool: &PgPool,
    library_id: LibraryId,
    series_id: SeriesID,
) -> Result<(bool, u64)> {
    let (finalized, version): (bool, i64) = sqlx::query_as(
        r#"
        SELECT finalized, version
        FROM series_bundle_versioning
        WHERE library_id = $1 AND series_id = $2
        "#,
    )
    .bind(library_id.to_uuid())
    .bind(series_id.to_uuid())
    .fetch_one(pool)
    .await
    .context("fetch series bundle versioning row")?;

    Ok((finalized, version as u64))
}

async fn publish_scan_event(state: &AppState, event: ScanEvent) -> Result<()> {
    state
        .scan_control()
        .orchestrator()
        .runtime()
        .events()
        .publish_scan_event(event)
        .await
        .context("publish scan event")
}

fn finalized_series_bundle_count(
    state: &AppState,
    library_id: LibraryId,
    series_id: SeriesID,
) -> usize {
    state
        .scan_control()
        .media_event_history_since_sequence(0)
        .into_iter()
        .filter(|frame| {
            matches!(
                &frame.event,
                MediaEvent::SeriesBundleFinalized {
                    library_id: event_library_id,
                    series_id: event_series_id,
                } if *event_library_id == library_id && *event_series_id == series_id
            )
        })
        .count()
}

async fn assert_no_series_bundle_finalized(
    state: &AppState,
    library_id: LibraryId,
    series_id: SeriesID,
    context: &str,
) {
    tokio::time::sleep(Duration::from_millis(75)).await;
    assert_eq!(
        finalized_series_bundle_count(state, library_id, series_id),
        0,
        "series bundle finalized too early after {context}"
    );
}

async fn wait_for_one_series_bundle_finalized(
    state: &AppState,
    library_id: LibraryId,
    series_id: SeriesID,
) {
    for _ in 0..100 {
        let count = finalized_series_bundle_count(state, library_id, series_id);
        assert!(count <= 1, "duplicate finalization events observed");
        if count == 1 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    panic!("timed out waiting for SeriesBundleFinalized");
}

fn media_discovered_event(
    library_id: LibraryId,
    series_root: &SeriesRootPath,
    season_context: &SeasonFolderScanContext,
    episode_id: EpisodeID,
    episode_path: &str,
) -> ScanEvent {
    ScanEvent::MediaFileDiscovered(Box::new(MediaFileDiscovered {
        library_id,
        path_norm: episode_path.to_string(),
        fingerprint: MediaFingerprint::default(),
        classified_as: MediaKindHint::Episode,
        media_id: MediaID::Episode(episode_id),
        variant: VideoMediaType::Episode,
        node: ScanNodeKind::EpisodeFile,
        hierarchy: AnalyzeScanHierarchy::Episode(EpisodeScanHierarchy {
            series_root_path: series_root.clone(),
            series: SeriesLink::Hint(SeriesHint {
                title: "Finalized Contract Series".to_string(),
                slug: Some("finalized-contract".to_string()),
                year: None,
                region: None,
            }),
            season: SeasonLink::Number(season_context.season_number),
            episode: EpisodeLink::Hint(EpisodeHint {
                number: 1,
                title: Some("Pilot".to_string()),
            }),
        }),
        context: FolderScanContext::Season(season_context.clone()),
        scan_reason: ScanReason::BulkSeed,
    }))
}

fn folder_completed_event(
    context: FolderScanContext,
    discovered_files: usize,
    enqueued_subfolders: usize,
    listing_hash: &str,
) -> ScanEvent {
    ScanEvent::FolderScanCompleted(FolderScanSummary {
        context,
        discovered_files,
        enqueued_subfolders,
        listing_hash: listing_hash.to_string(),
        outcome: FolderScanOutcome::Changed,
        completed_at: Utc::now(),
    })
}

fn indexed_series_event(
    library_id: LibraryId,
    series_root: &SeriesRootPath,
    series_id: SeriesID,
) -> ScanEvent {
    ScanEvent::Indexed(Box::new(IndexingOutcome {
        library_id,
        path_norm: series_root.as_str().to_string(),
        media_id: MediaID::Series(series_id),
        hierarchy: AnalyzeScanHierarchy::Series(SeriesScanHierarchy {
            series_root_path: series_root.clone(),
            series: SeriesLink::Resolved(SeriesRef {
                id: series_id,
                slug: Some("finalized-contract".to_string()),
                title: Some("Finalized Contract Series".to_string()),
            }),
        }),
        indexed_at: Utc::now(),
        upserted: true,
        media: None,
        change: IndexingChange::Created,
    }))
}

fn indexed_season_event(
    library_id: LibraryId,
    series_root: &SeriesRootPath,
    season_id: SeasonID,
    series_id: SeriesID,
    season_number: u16,
    season_path: &SeasonFolderPath,
) -> ScanEvent {
    ScanEvent::Indexed(Box::new(IndexingOutcome {
        library_id,
        path_norm: season_path.as_str().to_string(),
        media_id: MediaID::Season(season_id),
        hierarchy: AnalyzeScanHierarchy::Season(SeasonScanHierarchy {
            series_root_path: series_root.clone(),
            series: SeriesLink::Resolved(SeriesRef {
                id: series_id,
                slug: Some("finalized-contract".to_string()),
                title: Some("Finalized Contract Series".to_string()),
            }),
            season: SeasonLink::Resolved(SeasonRef {
                id: season_id,
                number: Some(season_number),
            }),
        }),
        indexed_at: Utc::now(),
        upserted: true,
        media: None,
        change: IndexingChange::Created,
    }))
}

fn indexed_episode_event(
    library_id: LibraryId,
    series_root: &SeriesRootPath,
    season_id: SeasonID,
    series_id: SeriesID,
    episode_id: EpisodeID,
    season_number: u16,
    episode_path: &str,
) -> ScanEvent {
    ScanEvent::Indexed(Box::new(IndexingOutcome {
        library_id,
        path_norm: episode_path.to_string(),
        media_id: MediaID::Episode(episode_id),
        hierarchy: AnalyzeScanHierarchy::Episode(EpisodeScanHierarchy {
            series_root_path: series_root.clone(),
            series: SeriesLink::Resolved(SeriesRef {
                id: series_id,
                slug: Some("finalized-contract".to_string()),
                title: Some("Finalized Contract Series".to_string()),
            }),
            season: SeasonLink::Resolved(SeasonRef {
                id: season_id,
                number: Some(season_number),
            }),
            episode: EpisodeLink::Resolved(EpisodeRef {
                id: episode_id,
                number: Some(1),
                title: Some("Pilot".to_string()),
            }),
        }),
        indexed_at: Utc::now(),
        upserted: true,
        media: None,
        change: IndexingChange::Created,
    }))
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn libraries_negotiate_json_rkyv_and_flatbuffers(
    pool: PgPool,
) -> Result<()> {
    let (server, state, _tempdir) = build_server(pool).await?;
    let token = login_test_user(&server, "media_fb_libraries").await?;
    create_library(&state, make_library("Movies", LibraryType::Movies)).await?;
    create_library(&state, make_library("Shows", LibraryType::Series)).await?;

    let json_response = server
        .get(v1::libraries::COLLECTION)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", "application/json")
        .await;
    json_response.assert_status_ok();
    assert_content_type_starts_with(&json_response, "application/json")?;
    let json_body: Value = json_response.json();
    assert_eq!(json_body["data"].as_array().expect("libraries").len(), 2);

    let default_response = server
        .get(v1::libraries::COLLECTION)
        .add_header("Authorization", bearer(&token))
        .await;
    default_response.assert_status_ok();
    assert_content_type_starts_with(&default_response, "application/json")?;
    let default_body: Value = default_response.json();
    assert_eq!(default_body["data"].as_array().expect("libraries").len(), 2);

    let rkyv_response = server
        .get(v1::libraries::COLLECTION)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", "application/octet-stream")
        .await;
    rkyv_response.assert_status_ok();
    assert_content_type_starts_with(
        &rkyv_response,
        "application/octet-stream",
    )?;
    let rkyv_bytes = aligned(rkyv_response.as_bytes().as_ref());
    let rkyv_libraries =
        rkyv::from_bytes::<Vec<Library>, rkyv::rancor::Error>(&rkyv_bytes)?;
    assert_eq!(rkyv_libraries.len(), 2);

    let fb_response = server
        .get(v1::libraries::COLLECTION)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", FLATBUFFERS_MIME)
        .await;
    fb_response.assert_status_ok();
    assert_content_type_starts_with(&fb_response, FLATBUFFERS_MIME)?;
    let list = flatbuffers::root::<fb::library::LibraryList>(
        fb_response.as_bytes().as_ref(),
    )?;
    let items = list.items().expect("library items");
    assert_eq!(items.len(), 2);
    assert!(items.iter().any(|library| library.name() == "Movies"));
    assert!(items.iter().any(|library| library.name() == "Shows"));

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn movie_batches_support_flatbuffers_and_preserve_rkyv(
    pool: PgPool,
) -> Result<()> {
    let (server, state, _tempdir) = build_server(pool.clone()).await?;
    let token = login_test_user(&server, "media_fb_movies").await?;
    let (library_id, movie_id, batch_id) = seed_movie(&state, &pool).await?;

    let sync_path = replace_param(
        v1::libraries::movie_batches::SYNC,
        "{id}",
        library_id.to_uuid().to_string(),
    );
    let sync = server
        .post(&sync_path)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", FLATBUFFERS_MIME)
        .content_type(FLATBUFFERS_MIME)
        .bytes(Bytes::from(batch_sync::serialize_batch_sync_request(&[])))
        .await;
    sync.assert_status_ok();
    assert_content_type_starts_with(&sync, FLATBUFFERS_MIME)?;
    let sync_response = flatbuffers::root::<fb::library::BatchSyncResponse>(
        sync.as_bytes().as_ref(),
    )?;
    assert_eq!(
        sync_response.stale_batch_ids().expect("stale").get(0),
        batch_id
    );
    assert_eq!(
        sync_response
            .server_versions()
            .expect("versions")
            .get(0)
            .batch_id(),
        batch_id
    );

    let item_path = replace_param(
        &replace_param(
            v1::libraries::movie_batches::ITEM,
            "{id}",
            library_id.to_uuid().to_string(),
        ),
        "{batch_id}",
        batch_id.to_string(),
    );
    let fb_item = server
        .get(&item_path)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", FLATBUFFERS_MIME)
        .await;
    fb_item.assert_status_ok();
    assert_content_type_starts_with(&fb_item, FLATBUFFERS_MIME)?;
    let batch = flatbuffers::root::<fb::library::MediaBatchData>(
        fb_item.as_bytes().as_ref(),
    )?;
    assert_eq!(batch.batch_id(), batch_id);
    let media = batch.items().expect("batch items").get(0);
    let movie = media.variant_as_movie_reference().expect("movie reference");
    assert_eq!(fb_to_uuid(movie.id()), movie_id.to_uuid());
    assert_eq!(movie.title(), "Test Movie");

    let fetch_path = replace_param(
        v1::libraries::movie_batches::FETCH,
        "{id}",
        library_id.to_uuid().to_string(),
    );
    let fetch = server
        .post(&fetch_path)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", FLATBUFFERS_MIME)
        .content_type(FLATBUFFERS_MIME)
        .bytes(Bytes::from(batch_sync::serialize_batch_fetch_request(&[
            batch_id,
        ])))
        .await;
    fetch.assert_status_ok();
    assert_content_type_starts_with(&fetch, FLATBUFFERS_MIME)?;
    let fetch_response = flatbuffers::root::<fb::library::BatchFetchResponse>(
        fetch.as_bytes().as_ref(),
    )?;
    assert_eq!(fetch_response.batches().expect("batches").len(), 1);

    let collection_path = replace_param(
        v1::libraries::movie_batches::COLLECTION,
        "{id}",
        library_id.to_uuid().to_string(),
    );
    let fb_collection = server
        .get(&collection_path)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", FLATBUFFERS_MIME)
        .await;
    fb_collection.assert_status_ok();
    assert_content_type_starts_with(&fb_collection, FLATBUFFERS_MIME)?;
    let collection = flatbuffers::root::<fb::library::BatchFetchResponse>(
        fb_collection.as_bytes().as_ref(),
    )?;
    let collection_batches = collection.batches().expect("collection batches");
    assert_eq!(collection_batches.len(), 1);
    assert_eq!(collection_batches.get(0).batch_id(), batch_id);

    let rkyv_collection = server
        .get(&collection_path)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", "application/octet-stream")
        .await;
    rkyv_collection.assert_status_ok();
    assert_content_type_starts_with(
        &rkyv_collection,
        "application/octet-stream",
    )?;
    let rkyv_collection_bytes = aligned(rkyv_collection.as_bytes().as_ref());
    let rkyv_bundle = rkyv::from_bytes::<
        ferrex_core::api::types::MovieReferenceBatchBundleResponse,
        rkyv::rancor::Error,
    >(&rkyv_collection_bytes)?;
    assert_eq!(rkyv_bundle.batches.len(), 1);
    assert_eq!(rkyv_bundle.batches[0].batch_id.as_u32(), batch_id);

    let rkyv_item = server
        .get(&item_path)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", "application/octet-stream")
        .await;
    rkyv_item.assert_status_ok();
    assert_content_type_starts_with(&rkyv_item, "application/octet-stream")?;
    let rkyv_bytes = aligned(rkyv_item.as_bytes().as_ref());
    let rkyv_batch = rkyv::from_bytes::<
        ferrex_core::api::types::MovieReferenceBatchResponse,
        rkyv::rancor::Error,
    >(&rkyv_bytes)?;
    assert_eq!(rkyv_batch.batch_id.as_u32(), batch_id);
    assert_eq!(rkyv_batch.movies[0].id, movie_id);

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn series_bundles_support_flatbuffers_and_preserve_rkyv(
    pool: PgPool,
) -> Result<()> {
    let (server, state, _tempdir) = build_server(pool.clone()).await?;
    let token = login_test_user(&server, "media_fb_series").await?;
    let (library_id, series_id, _season_id, _episode_id) =
        seed_series(&state, &pool).await?;

    let item_path = replace_param(
        &replace_param(
            v1::libraries::series_bundles::ITEM,
            "{id}",
            library_id.to_uuid().to_string(),
        ),
        "{series_id}",
        series_id.to_uuid().to_string(),
    );
    let fb_item = server
        .get(&item_path)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", FLATBUFFERS_MIME)
        .await;
    fb_item.assert_status_ok();
    assert_content_type_starts_with(&fb_item, FLATBUFFERS_MIME)?;
    let bundle = flatbuffers::root::<fb::library::SeriesBundleData>(
        fb_item.as_bytes().as_ref(),
    )?;
    assert_eq!(fb_to_uuid(bundle.series_id()), series_id.to_uuid());
    let items = bundle.items().expect("series bundle items");
    assert_eq!(items.len(), 3);
    assert_eq!(
        items
            .get(0)
            .variant_as_series_reference()
            .expect("series")
            .title(),
        "Test Series"
    );

    let sync_path = replace_param(
        v1::libraries::series_bundles::SYNC,
        "{id}",
        library_id.to_uuid().to_string(),
    );
    let sync = server
        .post(&sync_path)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", FLATBUFFERS_MIME)
        .content_type(FLATBUFFERS_MIME)
        .bytes(Bytes::from(
            batch_sync::serialize_series_bundle_sync_request(&[]),
        ))
        .await;
    sync.assert_status_ok();
    assert_content_type_starts_with(&sync, FLATBUFFERS_MIME)?;
    let sync_response = flatbuffers::root::<
        fb::library::SeriesBundleSyncResponse,
    >(sync.as_bytes().as_ref())?;
    assert_eq!(
        fb_to_uuid(&sync_response.stale_series_ids().expect("stale").get(0)),
        series_id.to_uuid()
    );

    let fetch_path = replace_param(
        v1::libraries::series_bundles::FETCH,
        "{id}",
        library_id.to_uuid().to_string(),
    );
    let fetch = server
        .post(&fetch_path)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", FLATBUFFERS_MIME)
        .content_type(FLATBUFFERS_MIME)
        .bytes(Bytes::from(
            batch_sync::serialize_series_bundle_fetch_request(&[
                series_id.to_uuid()
            ]),
        ))
        .await;
    fetch.assert_status_ok();
    assert_content_type_starts_with(&fetch, FLATBUFFERS_MIME)?;
    let fetch_response = flatbuffers::root::<
        fb::library::SeriesBundleFetchResponse,
    >(fetch.as_bytes().as_ref())?;
    assert_eq!(fetch_response.bundles().expect("bundles").len(), 1);

    let collection_path = replace_param(
        v1::libraries::series_bundles::COLLECTION,
        "{id}",
        library_id.to_uuid().to_string(),
    );
    let fb_collection = server
        .get(&collection_path)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", FLATBUFFERS_MIME)
        .await;
    fb_collection.assert_status_ok();
    assert_content_type_starts_with(&fb_collection, FLATBUFFERS_MIME)?;
    let collection = flatbuffers::root::<fb::library::SeriesBundleFetchResponse>(
        fb_collection.as_bytes().as_ref(),
    )?;
    let collection_bundles = collection.bundles().expect("collection bundles");
    assert_eq!(collection_bundles.len(), 1);
    assert_eq!(
        fb_to_uuid(collection_bundles.get(0).series_id()),
        series_id.to_uuid()
    );

    let rkyv_item = server
        .get(&item_path)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", "application/octet-stream")
        .await;
    rkyv_item.assert_status_ok();
    assert_content_type_starts_with(&rkyv_item, "application/octet-stream")?;
    let rkyv_bytes = aligned(rkyv_item.as_bytes().as_ref());
    let rkyv_bundle = rkyv::from_bytes::<
        ferrex_core::api::types::SeriesBundleResponse,
        rkyv::rancor::Error,
    >(&rkyv_bytes)?;
    assert_eq!(rkyv_bundle.series_id, series_id);
    assert_eq!(rkyv_bundle.seasons.len(), 1);
    assert_eq!(rkyv_bundle.episodes.len(), 1);

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn series_bundle_finalization_emits_once_and_feeds_sync_fetch(
    pool: PgPool,
) -> Result<()> {
    let (server, state, _tempdir) = build_server(pool.clone()).await?;
    let token = login_test_user(&server, "media_fb_series_finalize").await?;
    let library_id = create_library(
        &state,
        make_library("Finalization", LibraryType::Series),
    )
    .await?;
    let series_id = SeriesID(Uuid::now_v7());
    let season_id = SeasonID(Uuid::now_v7());
    let episode_id = EpisodeID(Uuid::now_v7());
    let series_root = SeriesRootPath::try_new(
        "/test/Finalization/Finalized Contract Series",
    )?;
    let (season_path, season_number) =
        SeasonFolderPath::try_new_under_series_root(
            &series_root,
            "/test/Finalization/Finalized Contract Series/Season 1",
        )?;
    let episode_path = format!("{}/S01E01.mkv", season_path.as_str());
    let series_context = SeriesFolderScanContext {
        library_id,
        series_root_path: series_root.clone(),
    };
    let season_context = SeasonFolderScanContext {
        library_id,
        series_root_path: series_root.clone(),
        season_folder_path: season_path.clone(),
        season_number,
    };

    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(versioning_row_count(&pool, library_id).await?, 0);

    publish_scan_event(
        &state,
        ScanEvent::FolderDiscovered {
            context: Box::new(FolderScanContext::Series(
                series_context.clone(),
            )),
            reason: ScanReason::BulkSeed,
            correlation_id: None,
            durable_job_id: None,
        },
    )
    .await?;
    assert_no_series_bundle_finalized(
        &state,
        library_id,
        series_id,
        "series root discovery",
    )
    .await;

    publish_scan_event(
        &state,
        ScanEvent::FolderDiscovered {
            context: Box::new(FolderScanContext::Season(
                season_context.clone(),
            )),
            reason: ScanReason::BulkSeed,
            correlation_id: None,
            durable_job_id: None,
        },
    )
    .await?;
    assert_no_series_bundle_finalized(
        &state,
        library_id,
        series_id,
        "season discovery",
    )
    .await;

    publish_scan_event(
        &state,
        media_discovered_event(
            library_id,
            &series_root,
            &season_context,
            episode_id,
            &episode_path,
        ),
    )
    .await?;
    assert_no_series_bundle_finalized(
        &state,
        library_id,
        series_id,
        "episode discovery",
    )
    .await;

    publish_scan_event(
        &state,
        folder_completed_event(
            FolderScanContext::Season(season_context.clone()),
            1,
            0,
            "season-listing-v1",
        ),
    )
    .await?;
    assert_no_series_bundle_finalized(
        &state,
        library_id,
        series_id,
        "season completion before root completion",
    )
    .await;

    publish_scan_event(
        &state,
        folder_completed_event(
            FolderScanContext::Series(series_context.clone()),
            0,
            1,
            "series-listing-v1",
        ),
    )
    .await?;
    assert_no_series_bundle_finalized(
        &state,
        library_id,
        series_id,
        "root completion before index readiness",
    )
    .await;

    publish_scan_event(
        &state,
        indexed_series_event(library_id, &series_root, series_id),
    )
    .await?;
    publish_scan_event(
        &state,
        indexed_season_event(
            library_id,
            &series_root,
            season_id,
            series_id,
            season_number,
            &season_path,
        ),
    )
    .await?;
    assert_no_series_bundle_finalized(
        &state,
        library_id,
        series_id,
        "series and season indexing before episode indexing",
    )
    .await;

    publish_scan_event(
        &state,
        indexed_episode_event(
            library_id,
            &series_root,
            season_id,
            series_id,
            episode_id,
            season_number,
            &episode_path,
        ),
    )
    .await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        finalized_series_bundle_count(&state, library_id, series_id),
        0,
        "finalization must wait for DB-hydratable series, season, and episode rows"
    );
    assert_eq!(versioning_row_count(&pool, library_id).await?, 0);

    seed_series_records(
        &state,
        &pool,
        library_id,
        series_id,
        season_id,
        episode_id,
        &episode_path,
    )
    .await?;

    // Replay the completed scan/index contract after DB readiness. This keeps
    // the assertion deterministic even if the background aggregator subscribed
    // after one of the intentionally pre-DB events above.
    publish_scan_event(
        &state,
        ScanEvent::FolderDiscovered {
            context: Box::new(FolderScanContext::Series(
                series_context.clone(),
            )),
            reason: ScanReason::BulkSeed,
            correlation_id: None,
            durable_job_id: None,
        },
    )
    .await?;
    publish_scan_event(
        &state,
        ScanEvent::FolderDiscovered {
            context: Box::new(FolderScanContext::Season(
                season_context.clone(),
            )),
            reason: ScanReason::BulkSeed,
            correlation_id: None,
            durable_job_id: None,
        },
    )
    .await?;
    publish_scan_event(
        &state,
        media_discovered_event(
            library_id,
            &series_root,
            &season_context,
            episode_id,
            &episode_path,
        ),
    )
    .await?;
    publish_scan_event(
        &state,
        folder_completed_event(
            FolderScanContext::Season(season_context.clone()),
            1,
            0,
            "season-listing-v1-repeat",
        ),
    )
    .await?;
    publish_scan_event(
        &state,
        folder_completed_event(
            FolderScanContext::Series(series_context.clone()),
            0,
            1,
            "series-listing-v1-repeat",
        ),
    )
    .await?;
    publish_scan_event(
        &state,
        indexed_series_event(library_id, &series_root, series_id),
    )
    .await?;
    publish_scan_event(
        &state,
        indexed_season_event(
            library_id,
            &series_root,
            season_id,
            series_id,
            season_number,
            &season_path,
        ),
    )
    .await?;
    publish_scan_event(
        &state,
        indexed_episode_event(
            library_id,
            &series_root,
            season_id,
            series_id,
            episode_id,
            season_number,
            &episode_path,
        ),
    )
    .await?;
    wait_for_one_series_bundle_finalized(&state, library_id, series_id).await;

    let (finalized, version) =
        series_bundle_version_row(&pool, library_id, series_id).await?;
    assert!(finalized);
    assert!(version >= 1);
    assert_eq!(versioning_row_count(&pool, library_id).await?, 1);

    publish_scan_event(
        &state,
        folder_completed_event(
            FolderScanContext::Season(season_context.clone()),
            1,
            0,
            "season-listing-v1-repeat",
        ),
    )
    .await?;
    publish_scan_event(
        &state,
        indexed_episode_event(
            library_id,
            &series_root,
            season_id,
            series_id,
            episode_id,
            season_number,
            &episode_path,
        ),
    )
    .await?;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        finalized_series_bundle_count(&state, library_id, series_id),
        1,
        "completion/index repeats must not emit duplicate finalizations"
    );
    assert_eq!(versioning_row_count(&pool, library_id).await?, 1);
    assert_eq!(
        series_bundle_version_row(&pool, library_id, series_id).await?,
        (true, version),
        "idempotent events must not alter stable bundle version rows"
    );

    let sync =
        post_series_bundle_sync_flatbuffers(&server, &token, library_id, &[])
            .await?;
    assert_eq!(sync.stale_series_ids, vec![series_id.to_uuid()]);
    assert!(sync.deleted_series_ids.is_empty());
    assert_eq!(
        sync.server_versions,
        vec![batch_sync::SeriesBundleVersion {
            series_id: series_id.to_uuid(),
            version,
        }]
    );

    let fetch_path = replace_param(
        v1::libraries::series_bundles::FETCH,
        "{id}",
        library_id.to_uuid().to_string(),
    );
    let fetch = server
        .post(&fetch_path)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", FLATBUFFERS_MIME)
        .content_type(FLATBUFFERS_MIME)
        .bytes(Bytes::from(
            batch_sync::serialize_series_bundle_fetch_request(&[
                series_id.to_uuid()
            ]),
        ))
        .await;
    fetch.assert_status_ok();
    assert_content_type_starts_with(&fetch, FLATBUFFERS_MIME)?;
    let response = flatbuffers::root::<fb::library::SeriesBundleFetchResponse>(
        fetch.as_bytes().as_ref(),
    )?;
    let bundles = response.bundles().context("missing bundles")?;
    assert_eq!(bundles.len(), 1);
    let bundle = bundles.get(0);
    assert_eq!(fb_to_uuid(bundle.series_id()), series_id.to_uuid());
    assert_eq!(bundle.version(), version);
    let items = bundle.items().context("missing bundle items")?;
    assert_eq!(items.len(), 3);
    let series_record = items
        .get(0)
        .variant_as_series_reference()
        .context("missing series record")?;
    assert_eq!(fb_to_uuid(series_record.id()), series_id.to_uuid());
    assert_eq!(series_record.title(), "Finalized Contract Series");
    let season_record = items
        .get(1)
        .variant_as_season_reference()
        .context("missing season record")?;
    assert_eq!(fb_to_uuid(season_record.id()), season_id.to_uuid());
    assert_eq!(season_record.season_number(), season_number);
    let episode_record = items
        .get(2)
        .variant_as_episode_reference()
        .context("missing episode record")?;
    assert_eq!(fb_to_uuid(episode_record.id()), episode_id.to_uuid());
    assert_eq!(episode_record.season_number(), season_number);
    assert_eq!(episode_record.episode_number(), 1);

    assert_eq!(versioning_row_count(&pool, library_id).await?, 1);
    assert_eq!(
        series_bundle_version_row(&pool, library_id, series_id).await?,
        (true, version),
        "sync/fetch should preserve the version when bundle bytes are unchanged"
    );

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn series_bundle_sync_flatbuffers_manifests_are_complete_and_repair_versioning(
    pool: PgPool,
) -> Result<()> {
    let (server, state, _tempdir) = build_server(pool.clone()).await?;
    let token = login_test_user(&server, "media_fb_series_sync_large").await?;
    let (library_id, series_ids) =
        seed_large_series_library(&state, &pool, LARGE_SERIES_COUNT).await?;
    let expected_series_ids = sorted_series_uuids(&series_ids);

    assert_eq!(expected_series_ids.len(), LARGE_SERIES_COUNT);
    assert_eq!(versioning_row_count(&pool, library_id).await?, 0);

    let empty_manifest =
        post_series_bundle_sync_flatbuffers(&server, &token, library_id, &[])
            .await?;
    assert_eq!(empty_manifest.stale_series_ids, expected_series_ids);
    assert!(empty_manifest.deleted_series_ids.is_empty());
    assert_eq!(empty_manifest.server_versions.len(), LARGE_SERIES_COUNT);
    assert_eq!(
        series_version_ids(&empty_manifest.server_versions),
        expected_series_ids
    );
    assert!(
        empty_manifest
            .server_versions
            .iter()
            .all(|version| version.version >= 1),
        "all repaired server versions should be non-zero"
    );

    let repaired_versions = series_bundle_versions(&state, library_id).await?;
    assert_eq!(repaired_versions, empty_manifest.server_versions);
    assert_eq!(
        versioning_row_count(&pool, library_id).await?,
        LARGE_SERIES_COUNT as i64
    );

    let repeated_empty_manifest =
        post_series_bundle_sync_flatbuffers(&server, &token, library_id, &[])
            .await?;
    assert_eq!(repeated_empty_manifest, empty_manifest);
    assert_eq!(
        series_bundle_versions(&state, library_id).await?,
        repaired_versions
    );

    let partial_client_manifest = repaired_versions
        .iter()
        .take(17)
        .copied()
        .collect::<Vec<_>>();
    let partial_sync = post_series_bundle_sync_flatbuffers(
        &server,
        &token,
        library_id,
        &partial_client_manifest,
    )
    .await?;
    let expected_partial_versions = repaired_versions[17..].to_vec();
    assert_eq!(
        partial_sync.stale_series_ids,
        series_version_ids(&expected_partial_versions)
    );
    assert!(partial_sync.deleted_series_ids.is_empty());
    assert_eq!(partial_sync.server_versions, expected_partial_versions);

    let stale_indices = [0, LARGE_SERIES_COUNT / 2, LARGE_SERIES_COUNT - 1];
    let mut stale_client_manifest = repaired_versions.clone();
    for index in stale_indices {
        stale_client_manifest[index].version =
            stale_client_manifest[index].version.saturating_sub(1);
    }
    let expected_stale_versions = stale_indices
        .iter()
        .map(|index| repaired_versions[*index])
        .collect::<Vec<_>>();
    let stale_sync = post_series_bundle_sync_flatbuffers(
        &server,
        &token,
        library_id,
        &stale_client_manifest,
    )
    .await?;
    assert_eq!(
        stale_sync.stale_series_ids,
        series_version_ids(&expected_stale_versions)
    );
    assert!(stale_sync.deleted_series_ids.is_empty());
    assert_eq!(stale_sync.server_versions, expected_stale_versions);

    let deleted_series_id = Uuid::now_v7();
    let mut deleted_client_manifest = repaired_versions.clone();
    deleted_client_manifest.push(batch_sync::SeriesBundleVersion {
        series_id: deleted_series_id,
        version: 99,
    });
    let deleted_sync = post_series_bundle_sync_flatbuffers(
        &server,
        &token,
        library_id,
        &deleted_client_manifest,
    )
    .await?;
    assert!(deleted_sync.stale_series_ids.is_empty());
    assert_eq!(deleted_sync.deleted_series_ids, vec![deleted_series_id]);
    assert!(deleted_sync.server_versions.is_empty());

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn series_bundle_fetch_and_collection_flatbuffers_are_complete_for_large_library(
    pool: PgPool,
) -> Result<()> {
    let (server, state, _tempdir) = build_server(pool.clone()).await?;
    let token = login_test_user(&server, "media_fb_series_fetch_large").await?;
    let (library_id, series_ids) =
        seed_large_series_library(&state, &pool, LARGE_SERIES_COUNT).await?;
    let expected_series_ids = sorted_series_uuids(&series_ids);

    let collection_path = replace_param(
        v1::libraries::series_bundles::COLLECTION,
        "{id}",
        library_id.to_uuid().to_string(),
    );
    let fb_collection = server
        .get(&collection_path)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", FLATBUFFERS_MIME)
        .await;
    fb_collection.assert_status_ok();
    assert_content_type_starts_with(&fb_collection, FLATBUFFERS_MIME)?;
    let collection =
        parse_series_bundle_fetch_response(fb_collection.as_bytes().as_ref())?;
    assert_eq!(collection.series_ids, expected_series_ids);
    assert!(collection.versions.iter().all(|version| *version >= 1));
    assert_eq!(collection.item_counts, vec![3; LARGE_SERIES_COUNT]);
    assert_eq!(
        collection.item_variants,
        vec![
            vec![
                fb::media::MediaVariant::SeriesReference,
                fb::media::MediaVariant::SeasonReference,
                fb::media::MediaVariant::EpisodeReference,
            ];
            LARGE_SERIES_COUNT
        ]
    );
    assert_unique_uuids(&collection.series_ids);

    let fetch_path = replace_param(
        v1::libraries::series_bundles::FETCH,
        "{id}",
        library_id.to_uuid().to_string(),
    );
    let requested_series_ids = vec![
        series_ids[17].to_uuid(),
        series_ids[3].to_uuid(),
        series_ids[17].to_uuid(),
        series_ids[65].to_uuid(),
        series_ids[0].to_uuid(),
        series_ids[LARGE_SERIES_COUNT - 1].to_uuid(),
        series_ids[42].to_uuid(),
        series_ids[9].to_uuid(),
        series_ids[64].to_uuid(),
    ];
    let expected_requested_ids =
        sorted_unique_uuids(requested_series_ids.clone());
    let fetch = server
        .post(&fetch_path)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", FLATBUFFERS_MIME)
        .content_type(FLATBUFFERS_MIME)
        .bytes(Bytes::from(
            batch_sync::serialize_series_bundle_fetch_request(
                &requested_series_ids,
            ),
        ))
        .await;
    fetch.assert_status_ok();
    assert_content_type_starts_with(&fetch, FLATBUFFERS_MIME)?;
    let fetched =
        parse_series_bundle_fetch_response(fetch.as_bytes().as_ref())?;
    assert_eq!(fetched.series_ids, expected_requested_ids);
    assert!(fetched.versions.iter().all(|version| *version >= 1));
    assert_eq!(fetched.item_counts, vec![3; expected_requested_ids.len()]);
    assert_eq!(
        fetched.item_variants,
        vec![
            vec![
                fb::media::MediaVariant::SeriesReference,
                fb::media::MediaVariant::SeasonReference,
                fb::media::MediaVariant::EpisodeReference,
            ];
            expected_requested_ids.len()
        ]
    );
    assert_unique_uuids(&fetched.series_ids);

    let missing_id = Uuid::now_v7();
    let missing = server
        .post(&fetch_path)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", FLATBUFFERS_MIME)
        .content_type(FLATBUFFERS_MIME)
        .bytes(Bytes::from(
            batch_sync::serialize_series_bundle_fetch_request(&[
                series_ids[0].to_uuid(),
                missing_id,
            ]),
        ))
        .await;
    missing.assert_status(StatusCode::NOT_FOUND);

    let invalid = server
        .post(&fetch_path)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", FLATBUFFERS_MIME)
        .content_type(FLATBUFFERS_MIME)
        .bytes(Bytes::from_static(b"not-a-flatbuffer"))
        .await;
    invalid.assert_status(StatusCode::BAD_REQUEST);

    Ok(())
}

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn images_support_flatbuffers_manifest_and_iid_pending(
    pool: PgPool,
) -> Result<()> {
    let (server, state, tempdir) = build_server(pool).await?;
    let token = login_test_user(&server, "media_fb_images").await?;
    let iid = Uuid::now_v7();

    let manifest = server
        .post(v1::images::MANIFEST)
        .add_header("Authorization", bearer(&token))
        .add_header("Accept", FLATBUFFERS_MIME)
        .content_type(FLATBUFFERS_MIME)
        .bytes(Bytes::from(fb_image_manifest_request(iid)))
        .await;
    manifest.assert_status_ok();
    assert_content_type_starts_with(&manifest, FLATBUFFERS_MIME)?;
    let manifest_response = flatbuffers::root::<
        fb::image::ImageManifestResponse,
    >(manifest.as_bytes().as_ref())?;
    let entry = manifest_response.entries().expect("entries").get(0);
    assert_eq!(fb_to_uuid(entry.iid()), iid);
    assert_eq!(entry.category(), fb::common::ImageCategory::Poster);
    assert_eq!(entry.status(), fb::image::ImageStatus::Pending);
    assert_eq!(entry.retry_after_millis(), 1_000);
    assert!(entry.token().is_none());

    let iid_path =
        replace_param(v1::images::IID_ITEM, "{iid}", iid.to_string());
    let pending = server
        .get(&iid_path)
        .add_header("Authorization", bearer(&token))
        .await;
    pending.assert_status(StatusCode::ACCEPTED);
    assert_eq!(
        pending
            .maybe_header(header::RETRY_AFTER)
            .context("Retry-After missing")?,
        "1"
    );
    let body = String::from_utf8(pending.as_bytes().to_vec())?;
    assert!(body.contains("pending"));
    assert!(!body.contains("/"));

    let ready_iid = Uuid::now_v7();
    let png_bytes = b"\x89PNG\r\n\x1a\nready";
    let image_store = ImageBlobStore::new(ImageCacheRoot::new(
        tempdir.path().join("cache/images"),
    ));
    let key = image_cache_key_for(ready_iid, ImageSize::poster());
    let stored = image_store.write(&key, png_bytes).await?;
    let blob_token =
        ImageFileStore::token_from_integrity(&stored.integrity.to_string());
    let blob_path = state.image_service().image_blob_path(&blob_token)?;
    if let Some(parent) = blob_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&blob_path, png_bytes).await?;

    let ready_path =
        replace_param(v1::images::IID_ITEM, "{iid}", ready_iid.to_string());
    let ready = server
        .get(&ready_path)
        .add_header("Authorization", bearer(&token))
        .await;
    ready.assert_status(StatusCode::TEMPORARY_REDIRECT);
    let location = ready
        .maybe_header(header::LOCATION)
        .context("Location missing")?
        .to_str()?
        .to_owned();
    let expected_location =
        v1::images::BLOB_ITEM.replace("{token}", &blob_token);
    assert_eq!(location, expected_location);
    assert!(!location.contains(tempdir.path().to_string_lossy().as_ref()));

    let blob = server
        .get(&location)
        .add_header("Authorization", bearer(&token))
        .await;
    blob.assert_status_ok();
    assert_content_type_starts_with(&blob, "image/png")?;
    assert_eq!(blob.as_bytes().as_ref(), png_bytes);

    Ok(())
}
