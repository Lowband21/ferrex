use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use axum::{Router, body::Bytes, http::StatusCode, http::header};
use axum_test::{TestResponse, TestServer};
use chrono::Utc;
use ferrex_core::{
    api::routes::{utils::replace_param, v1},
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
    MediaFile, MediaFileMetadata, MediaID, MovieID, MovieReference, SeasonID,
    SeasonReference, Series, SeriesID,
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
    let now = Utc::now();
    MediaFile {
        id: Uuid::now_v7(),
        media_id,
        path: PathBuf::from(format!("/media/{}.mkv", Uuid::now_v7())),
        filename: "file.mkv".to_string(),
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
    assert_eq!(entry.status(), fb::image::ImageStatus::Pending);
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
