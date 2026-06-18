//! Round-trip tests for Ferrex model/auth payloads serialized as FlatBuffers.

use chrono::Utc;
use ferrex_flatbuffers::conversions::{
    auth, batch_data, batch_sync, details, image, library,
};
use ferrex_flatbuffers::fb;
use ferrex_flatbuffers::uuid_helpers::{fb_to_uuid, uuid_to_fb};
use ferrex_model::library::LibraryLikeMut;
use ferrex_model::{
    EpisodeID, EpisodeReference, Library, LibraryId, LibraryType, MediaFile,
    MediaFileMetadata, MediaID, MovieBatchId, MovieID, MovieReference,
    SeasonID, SeasonReference, Series, SeriesID,
};
use std::path::PathBuf;
use uuid::Uuid;

fn make_test_library(name: &str, lib_type: LibraryType) -> Library {
    let mut lib = Library::new(
        name.to_string(),
        lib_type,
        vec![PathBuf::from("/media/movies")],
    );
    lib.scan_interval_minutes = 120;
    lib.enabled = true;
    lib.auto_scan = true;
    lib
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
        poster_path: Some("/poster.jpg".to_string()),
        backdrop_path: Some("/backdrop.jpg".to_string()),
        logo_path: None,
        primary_poster_iid: Some(Uuid::now_v7()),
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
        poster_path: Some("/series-poster.jpg".to_string()),
        backdrop_path: None,
        logo_path: None,
        primary_poster_iid: Some(Uuid::now_v7()),
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
        poster_path: Some("/season-poster.jpg".to_string()),
        primary_poster_iid: Some(Uuid::now_v7()),
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
        still_path: Some("/still.jpg".to_string()),
        primary_still_iid: Some(Uuid::now_v7()),
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

fn make_cast_credit(
    name: &str,
    character: &str,
    person_id: Uuid,
    profile_iid: Uuid,
) -> ferrex_model::details::CastMember {
    ferrex_model::details::CastMember {
        id: 7_001,
        person_id: Some(person_id),
        credit_id: Some(format!("{name}-credit")),
        cast_id: Some(17),
        name: name.to_string(),
        original_name: Some(format!("{name} Original")),
        character: character.to_string(),
        profile_path: Some(format!("/profiles/{name}.jpg")),
        order: 0,
        gender: Some(2),
        known_for_department: Some("Acting".to_string()),
        adult: Some(false),
        popularity: Some(12.5),
        also_known_as: Vec::new(),
        external_ids: ferrex_model::details::PersonExternalIds::default(),
        image_slot: 1,
        image_id: Some(profile_iid),
    }
}

fn make_crew_credit(
    name: &str,
    job: &str,
    department: &str,
    person_id: Uuid,
    profile_iid: Uuid,
) -> ferrex_model::details::CrewMember {
    ferrex_model::details::CrewMember {
        id: 8_001,
        person_id: Some(person_id),
        credit_id: Some(format!("{name}-credit")),
        name: name.to_string(),
        job: job.to_string(),
        department: department.to_string(),
        profile_path: Some(format!("/profiles/{name}.jpg")),
        gender: Some(1),
        known_for_department: Some(department.to_string()),
        adult: Some(false),
        popularity: Some(10.0),
        original_name: Some(format!("{name} Original")),
        also_known_as: Vec::new(),
        external_ids: ferrex_model::details::PersonExternalIds::default(),
        profile_iid: Some(profile_iid),
    }
}

fn make_media_file(media_id: MediaID, library_id: LibraryId) -> MediaFile {
    let now = Utc::now();
    MediaFile {
        id: Uuid::now_v7(),
        media_id,
        path: PathBuf::from("/media/file.mkv"),
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
            parsed_info: Some(ferrex_model::files::ParsedMediaInfo::Movie(
                ferrex_model::files::ParsedMovieInfo {
                    title: "File".to_string(),
                    year: Some(2026),
                    resolution: Some("1080p".to_string()),
                    source: Some("BluRay".to_string()),
                    release_group: Some("LOW".to_string()),
                },
            )),
        }),
        library_id,
    }
}

fn make_movie_reference() -> MovieReference {
    let library_id = LibraryId(Uuid::now_v7());
    let movie_id = MovieID(Uuid::now_v7());
    MovieReference {
        id: movie_id,
        library_id,
        batch_id: Some(MovieBatchId::new(7).expect("non-zero batch id")),
        tmdb_id: 100,
        title: "Test Movie".into(),
        details: make_movie_details("Test Movie"),
        endpoint: "/api/v1/stream/movie".to_string().into(),
        file: make_media_file(MediaID::Movie(movie_id), library_id),
        theme_color: Some("#112233".to_string()),
    }
}

fn make_series_bundle() -> (Series, Vec<SeasonReference>, Vec<EpisodeReference>)
{
    let library_id = LibraryId(Uuid::now_v7());
    let series_id = SeriesID(Uuid::now_v7());
    let season_id = SeasonID(Uuid::now_v7());
    let episode_id = EpisodeID(Uuid::now_v7());
    let now = Utc::now();

    let series = Series {
        id: series_id,
        library_id,
        tmdb_id: 200,
        title: "Test Series".into(),
        details: make_series_details("Test Series"),
        endpoint: "/api/v1/series/test".to_string().into(),
        discovered_at: now,
        created_at: now,
        theme_color: Some("#334455".to_string()),
    };
    let season = SeasonReference {
        id: season_id,
        library_id,
        season_number: 1.into(),
        series_id,
        tmdb_series_id: 200,
        details: make_season_details(),
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

    (series, vec![season], vec![episode])
}

#[test]
fn uuid_round_trips() {
    let original = Uuid::now_v7();
    let fb_uuid = uuid_to_fb(&original);
    assert_eq!(fb_to_uuid(&fb_uuid), original);
}

#[test]
fn image_manifest_conversions_round_trip() {
    let iid = Uuid::now_v7();
    let mut builder = flatbuffers::FlatBufferBuilder::new();
    let fb_iid = uuid_to_fb(&iid);
    let query = fb::image::ImageQuery::create(
        &mut builder,
        &fb::image::ImageQueryArgs {
            iid: Some(&fb_iid),
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
    let request_bytes = builder.finished_data().to_vec();

    let parsed = image::parse_image_manifest_request(&request_bytes)
        .expect("parse image manifest request");
    assert_eq!(parsed[0].iid, iid);
    assert_eq!(parsed[0].category, fb::common::ImageCategory::Poster);

    let response_bytes = image::serialize_image_manifest_response(&[
        image::ImageManifestEntry {
            iid,
            category: fb::common::ImageCategory::Poster,
            status: image::ImageManifestEntryStatus::Ready { token: "abc" },
        },
        image::ImageManifestEntry {
            iid,
            category: fb::common::ImageCategory::Backdrop,
            status: image::ImageManifestEntryStatus::Pending {
                retry_after_ms: 1_500,
            },
        },
        image::ImageManifestEntry {
            iid,
            category: fb::common::ImageCategory::Profile,
            status: image::ImageManifestEntryStatus::Failed {
                reason: Some("missing"),
            },
        },
    ]);
    let response =
        flatbuffers::root::<fb::image::ImageManifestResponse>(&response_bytes)
            .expect("image manifest response root");
    let entries = response.entries().expect("entries");
    let entry = entries.get(0);
    assert_eq!(fb_to_uuid(entry.iid()), iid);
    assert_eq!(entry.category(), fb::common::ImageCategory::Poster);
    assert_eq!(entry.status(), fb::image::ImageStatus::Ready);
    assert_eq!(entry.token(), Some("abc"));
    let pending = entries.get(1);
    assert_eq!(pending.category(), fb::common::ImageCategory::Backdrop);
    assert_eq!(pending.status(), fb::image::ImageStatus::Pending);
    assert_eq!(pending.retry_after_millis(), 1_500);
    let failed = entries.get(2);
    assert_eq!(failed.category(), fb::common::ImageCategory::Profile);
    assert_eq!(failed.status(), fb::image::ImageStatus::Failed);
    assert_eq!(failed.failure_reason(), Some("missing"));
}

#[test]
fn auth_token_round_trips_optional_ids_and_scope() {
    let session_id = Uuid::now_v7();
    let device_session_id = Uuid::now_v7();
    let user_id = Uuid::now_v7();
    let bytes = auth::serialize_auth_token(&auth::AuthToken {
        access_token: "access",
        refresh_token: "refresh",
        expires_in: 900,
        session_id: Some(session_id),
        device_session_id: Some(device_session_id),
        user_id: Some(user_id),
        scope: auth::SessionScope::Playback,
    });

    let token = flatbuffers::root::<fb::auth::AuthToken>(&bytes)
        .expect("auth token root");
    assert_eq!(token.access_token(), "access");
    assert_eq!(token.refresh_token(), "refresh");
    assert_eq!(token.expires_in(), 900);
    assert_eq!(token.scope(), fb::auth::SessionScope::Playback);
    assert_eq!(
        fb_to_uuid(token.session_id().expect("session id")),
        session_id
    );
    assert_eq!(
        fb_to_uuid(token.device_session_id().expect("device session id")),
        device_session_id
    );
    assert_eq!(fb_to_uuid(token.user_id().expect("user id")), user_id);
}

#[test]
fn device_auth_contracts_round_trip() {
    let device_id = Uuid::now_v7();
    let user_id = Uuid::now_v7();
    let session_id = Uuid::now_v7();
    let device_session_id = Uuid::now_v7();
    let challenge_id = Uuid::now_v7();
    let now = Utc::now();

    let request = auth::DeviceLoginRequest {
        username: "alice".into(),
        password: "correct horse battery staple".into(),
        device_info: Some(auth::DeviceInfo {
            device_id,
            device_name: "Alice Phone".into(),
            platform: auth::Platform::Android,
            app_version: "1.2.3".into(),
            hardware_id: Some("hardware-1".into()),
        }),
        remember_device: true,
        device_public_key: Some("base64-public-key".into()),
        device_key_alg: Some("ed25519".into()),
    };
    let parsed = auth::parse_device_login_request(
        &auth::serialize_device_login_request(&request),
    )
    .expect("parse device login request");
    assert_eq!(parsed, request);

    let challenge_request = auth::PinChallengeRequest { device_id };
    let parsed_challenge = auth::parse_pin_challenge_request(
        &auth::serialize_pin_challenge_request(&challenge_request),
    )
    .expect("parse pin challenge request");
    assert_eq!(parsed_challenge, challenge_request);

    let pin_login = auth::PinLoginRequest {
        device_id: device_session_id,
        client_proof: "pin-proof".into(),
        challenge_id,
        device_signature: "device-signature".into(),
    };
    let parsed_pin_login = auth::parse_pin_login_request(
        &auth::serialize_pin_login_request(&pin_login),
    )
    .expect("parse pin login request");
    assert_eq!(parsed_pin_login, pin_login);

    let response_bytes =
        auth::serialize_device_login_response(&auth::DeviceLoginResponse {
            access_token: "access",
            session_token: "access",
            refresh_token: "refresh",
            expires_in: 900,
            session_id: Some(session_id),
            device_session_id: Some(device_session_id),
            user_id: Some(user_id),
            scope: auth::SessionScope::Playback,
            device_registration: Some(auth::DeviceRegistration {
                id: device_session_id,
                user_id,
                device_id,
                device_name: "Alice Phone",
                platform: auth::Platform::Android,
                app_version: "1.2.3",
                pin_configured: true,
                registered_at: now,
                last_used_at: now,
                expires_at: None,
                revoked: false,
                revoked_by: None,
                revoked_at: None,
            }),
            requires_pin_setup: false,
        });
    let response =
        flatbuffers::root::<fb::auth::DeviceLoginResponse>(&response_bytes)
            .expect("device login response root");
    assert_eq!(response.access_token(), "access");
    assert_eq!(response.session_token(), "access");
    assert_eq!(response.refresh_token(), "refresh");
    assert_eq!(response.scope(), fb::auth::SessionScope::Playback);
    assert_eq!(
        fb_to_uuid(response.session_id().expect("session id")),
        session_id
    );
    assert_eq!(
        fb_to_uuid(response.device_session_id().expect("device session id")),
        device_session_id
    );
    let registration = response.device_registration().expect("registration");
    assert_eq!(fb_to_uuid(registration.id()), device_session_id);
    assert_eq!(registration.device_name(), "Alice Phone");
    assert_eq!(registration.platform(), fb::auth::Platform::Android);
    assert!(registration.expires_at().is_none());

    let challenge_response_bytes =
        auth::serialize_pin_challenge_response(&auth::PinChallengeResponse {
            challenge_id,
            nonce: "nonce-b64",
            expires_in_secs: 120,
            pin_salt: "salt-b64",
        });
    let challenge_response =
        flatbuffers::root::<fb::auth::PinChallengeResponse>(
            &challenge_response_bytes,
        )
        .expect("pin challenge response root");
    assert_eq!(fb_to_uuid(challenge_response.challenge_id()), challenge_id);
    assert_eq!(challenge_response.nonce(), "nonce-b64");
    assert_eq!(challenge_response.expires_in_secs(), 120);
    assert_eq!(challenge_response.pin_salt(), "salt-b64");

    let status_bytes =
        auth::serialize_device_auth_status(&auth::DeviceAuthStatus {
            device_registered: true,
            has_pin: true,
            remaining_attempts: Some(2),
        });
    let status = flatbuffers::root::<fb::auth::DeviceAuthStatus>(&status_bytes)
        .expect("device status root");
    assert!(status.device_registered());
    assert!(status.has_pin());
    assert_eq!(status.remaining_attempts(), Some(2));

    let known_request = auth::KnownDeviceProfilesRequest {
        device_info: Some(auth::DeviceInfo {
            device_id,
            device_name: "Alice Phone".into(),
            platform: auth::Platform::Android,
            app_version: "1.2.3".into(),
            hardware_id: None,
        }),
    };
    let parsed_known_request = auth::parse_known_device_profiles_request(
        &auth::serialize_known_device_profiles_request(&known_request),
    )
    .expect("parse known device profiles request");
    assert_eq!(parsed_known_request, known_request);

    let known_response_bytes = auth::serialize_known_device_profiles_response(
        true,
        &[auth::KnownDeviceUserCard {
            id: user_id,
            username: "alice",
            display_name: "Alice",
            avatar_url: Some("https://example.invalid/avatar.png"),
            has_pin: true,
        }],
    );
    let known_response = flatbuffers::root::<
        fb::auth::KnownDeviceProfilesResponse,
    >(&known_response_bytes)
    .expect("known profiles response root");
    assert!(known_response.known_device());
    let user_card = known_response.users().expect("users").get(0);
    assert_eq!(fb_to_uuid(user_card.id()), user_id);
    assert_eq!(user_card.username(), "alice");
    assert!(user_card.has_pin());

    let devices_bytes =
        auth::serialize_authenticated_devices(&[auth::AuthenticatedDevice {
            id: device_session_id,
            user_id,
            fingerprint: "fingerprint",
            name: "Alice Phone",
            platform: auth::Platform::Android,
            app_version: Some("1.2.3"),
            hardware_id: None,
            status: auth::AuthDeviceStatus::Trusted,
            pin_configured: true,
            failed_attempts: 1,
            locked_until: None,
            first_authenticated_by: user_id,
            first_authenticated_at: now,
            trusted_until: None,
            last_seen_at: now,
            last_activity: now,
            auto_login_enabled: true,
            revoked_by: None,
            revoked_at: None,
            revoked_reason: None,
            created_at: now,
            updated_at: now,
        }]);
    let devices = flatbuffers::root::<fb::auth::AuthenticatedDevicesResponse>(
        &devices_bytes,
    )
    .expect("authenticated devices response root");
    let device = devices.devices().expect("devices").get(0);
    assert_eq!(device.fingerprint(), "fingerprint");
    assert_eq!(device.status(), fb::auth::AuthDeviceStatus::Trusted);
    assert!(device.auto_login_enabled());
}

#[test]
fn setup_status_and_user_profile_round_trip() {
    let policy = auth::PasswordPolicy {
        enforce: true,
        min_length: 8,
        require_uppercase: true,
        require_lowercase: true,
        require_number: true,
        require_special: false,
    };
    let pin_policy = auth::PinPolicy {
        min_length: 5,
        max_length: 6,
        require_numeric: true,
        reject_repeated_digits: true,
        max_consecutive_identical: 2,
        reject_sequential_digits: false,
    };
    let device_trust_policy = auth::DeviceTrustPolicy {
        remember_device_default: true,
        trust_duration_days: 45,
        pin_max_attempts: 7,
        pin_lockout_minutes: 12,
        admin_pin_unlock_enabled: true,
    };
    let status_bytes = auth::serialize_setup_status(&auth::SetupStatus {
        needs_setup: false,
        has_admin: true,
        requires_setup_token: true,
        user_count: 2,
        library_count: 3,
        admin_password_policy: Some(policy),
        user_password_policy: Some(policy),
        pin_policy: Some(pin_policy),
        device_trust_policy: Some(device_trust_policy),
    });
    let status = flatbuffers::root::<fb::auth::SetupStatus>(&status_bytes)
        .expect("setup status root");
    assert!(!status.needs_setup());
    assert!(status.has_admin());
    assert!(status.requires_setup_token());
    assert_eq!(status.user_count(), 2);
    assert_eq!(status.library_count(), 3);
    assert_eq!(
        status
            .admin_password_policy()
            .expect("admin policy")
            .min_length(),
        8
    );
    assert_eq!(status.pin_policy().expect("pin policy").min_length(), 5);
    assert_eq!(status.pin_policy().expect("pin policy").max_length(), 6);
    assert!(
        status
            .device_trust_policy()
            .expect("device trust policy")
            .remember_device_default()
    );
    assert_eq!(
        status
            .device_trust_policy()
            .expect("device trust policy")
            .pin_max_attempts(),
        7
    );

    let id = Uuid::now_v7();
    let now = Utc::now();
    let profile_bytes = auth::serialize_user_profile(&auth::UserProfile {
        id,
        username: "alice",
        display_name: "Alice",
        avatar_url: Some("https://example.invalid/avatar.png"),
        email: Some("alice@example.invalid"),
        created_at: now,
        updated_at: now,
        last_login: Some(now),
        is_active: true,
    });
    let profile = flatbuffers::root::<fb::auth::UserProfile>(&profile_bytes)
        .expect("profile root");
    assert_eq!(fb_to_uuid(profile.id()), id);
    assert_eq!(profile.username(), "alice");
    assert_eq!(profile.display_name(), "Alice");
    assert_eq!(
        profile.avatar_url(),
        Some("https://example.invalid/avatar.png")
    );
    assert_eq!(profile.email(), Some("alice@example.invalid"));
    assert!(profile.last_login().expect("last login").millis() > 0);
}

#[test]
fn library_list_and_reference_list_round_trip() {
    let libraries = vec![
        make_test_library("Movies", LibraryType::Movies),
        make_test_library("TV Shows", LibraryType::Series),
    ];

    let bytes = library::serialize_library_list(&libraries);
    let list = flatbuffers::root::<fb::library::LibraryList>(&bytes)
        .expect("library list root");
    let items = list.items().expect("items");
    assert_eq!(items.len(), 2);
    let lib0 = items.get(0);
    assert_eq!(lib0.name(), "Movies");
    assert_eq!(lib0.library_type(), fb::common::LibraryType::Movies);
    assert_eq!(lib0.scan_interval_minutes(), 120);
    assert_eq!(lib0.movie_ref_batch_size(), 100);
    assert_eq!(fb_to_uuid(lib0.id()), *libraries[0].id.as_uuid());
    assert_eq!(lib0.paths().expect("paths").get(0), "/media/movies");

    let refs = vec![ferrex_model::details::LibraryReference {
        id: libraries[1].id,
        name: libraries[1].name.clone(),
        library_type: libraries[1].library_type,
        paths: libraries[1].paths.clone(),
    }];
    let ref_bytes = library::serialize_library_reference_list(&refs);
    let refs_list = flatbuffers::root::<fb::library::LibraryList>(&ref_bytes)
        .expect("reference list root");
    let ref_item = refs_list.items().expect("ref items").get(0);
    assert_eq!(ref_item.name(), "TV Shows");
    assert_eq!(ref_item.library_type(), fb::common::LibraryType::Series);
}

#[test]
fn movie_batch_sync_and_fetch_round_trip() {
    let sync_bytes =
        batch_sync::serialize_batch_sync_request(&[batch_sync::BatchVersion {
            batch_id: 7,
            version: 10,
        }]);
    let sync_request =
        flatbuffers::root::<fb::library::BatchSyncRequest>(&sync_bytes)
            .expect("sync request root");
    let cached = sync_request.cached_versions().expect("cached versions");
    assert_eq!(cached.len(), 1);
    assert_eq!(cached.get(0).batch_id(), 7);
    assert_eq!(cached.get(0).version(), 10);
    let parsed_sync =
        batch_sync::parse_batch_sync_request(&sync_bytes).expect("parse sync");
    assert_eq!(parsed_sync[0].batch_id, 7);
    assert_eq!(parsed_sync[0].version, 10);

    let sync_response_bytes = batch_sync::serialize_batch_sync_response(
        &[7],
        &[3],
        &[batch_sync::BatchVersion {
            batch_id: 7,
            version: 11,
        }],
    );
    let sync_response = flatbuffers::root::<fb::library::BatchSyncResponse>(
        &sync_response_bytes,
    )
    .expect("sync response root");
    assert_eq!(sync_response.stale_batch_ids().expect("stale").get(0), 7);
    assert_eq!(
        sync_response.deleted_batch_ids().expect("deleted").get(0),
        3
    );
    assert_eq!(
        sync_response
            .server_versions()
            .expect("versions")
            .get(0)
            .version(),
        11
    );

    let fetch_request_bytes =
        batch_sync::serialize_batch_fetch_request(&[7, 8]);
    let parsed_fetch =
        batch_sync::parse_batch_fetch_request(&fetch_request_bytes)
            .expect("parse fetch");
    assert_eq!(parsed_fetch, vec![7, 8]);

    let movie = make_movie_reference();
    let fetch_bytes =
        batch_data::serialize_batch_fetch_response(&[batch_data::MovieBatch {
            batch_id: 7,
            version: 11,
            movies: std::slice::from_ref(&movie),
        }]);
    let fetch =
        flatbuffers::root::<fb::library::BatchFetchResponse>(&fetch_bytes)
            .expect("fetch response root");
    let batch = fetch.batches().expect("batches").get(0);
    assert_eq!(batch.batch_id(), 7);
    assert_eq!(batch.version(), 11);
    let media = batch.items().expect("items").get(0);
    let movie = media
        .variant_as_movie_reference()
        .expect("movie reference variant");
    assert_eq!(movie.title(), "Test Movie");
    assert_eq!(movie.batch_id(), 7);
    assert_eq!(movie.file().expect("file").filename(), "file.mkv");
    let parsed = movie
        .file()
        .expect("file")
        .metadata()
        .expect("metadata")
        .parsed_info()
        .expect("parsed info")
        .variant_as_parsed_movie_info()
        .expect("parsed movie info");
    assert_eq!(parsed.year(), 2026);
}

#[test]
fn detail_credit_profile_and_related_fields_serialize() {
    let cast_person_id = Uuid::now_v7();
    let cast_profile_iid = Uuid::now_v7();
    let crew_person_id = Uuid::now_v7();
    let crew_profile_iid = Uuid::now_v7();
    let guest_profile_iid = Uuid::now_v7();

    let cast =
        make_cast_credit("Actor One", "Hero", cast_person_id, cast_profile_iid);
    let crew = make_crew_credit(
        "Director One",
        "Director",
        "Directing",
        crew_person_id,
        crew_profile_iid,
    );
    let related = ferrex_model::details::RelatedMediaRef {
        tmdb_id: 9_001,
        title: Some("Recommended Title".to_string()),
    };
    let similar = ferrex_model::details::RelatedMediaRef {
        tmdb_id: 9_002,
        title: Some("Similar Title".to_string()),
    };

    let mut movie = make_movie_details("Credit Movie");
    movie.cast = vec![cast.clone()];
    movie.crew = vec![crew.clone()];
    movie.recommendations = vec![related.clone()];
    movie.similar = vec![similar.clone()];
    let mut movie_builder = flatbuffers::FlatBufferBuilder::new();
    let movie_root =
        details::build_enhanced_movie_details(&mut movie_builder, &movie);
    movie_builder.finish(movie_root, None);
    let movie = flatbuffers::root::<fb::details::EnhancedMovieDetails>(
        movie_builder.finished_data(),
    )
    .expect("movie details root");
    let actor = movie.cast().expect("movie cast").get(0);
    assert_eq!(actor.name(), Some("Actor One"));
    assert_eq!(actor.character(), Some("Hero"));
    let actor_profile = actor.profile().expect("actor profile");
    assert_eq!(
        fb_to_uuid(actor_profile.person_id().expect("person id")),
        cast_person_id
    );
    assert_eq!(
        actor_profile.profile_path(),
        Some("/profiles/Actor One.jpg")
    );
    assert_eq!(
        fb_to_uuid(actor_profile.profile_iid().expect("profile iid")),
        cast_profile_iid
    );
    let movie_crew = movie.crew().expect("movie crew").get(0);
    assert_eq!(movie_crew.job(), Some("Director"));
    assert_eq!(
        fb_to_uuid(
            movie_crew
                .profile()
                .expect("crew profile")
                .profile_iid()
                .expect("crew profile iid"),
        ),
        crew_profile_iid
    );
    assert_eq!(
        movie
            .recommendations()
            .expect("recommendations")
            .get(0)
            .tmdb_id(),
        9_001
    );
    assert_eq!(
        movie.similar().expect("similar").get(0).title(),
        Some("Similar Title")
    );

    let mut series = make_series_details("Credit Series");
    series.cast = vec![cast];
    series.crew = vec![crew];
    series.recommendations = vec![related];
    series.similar = vec![similar];
    let mut series_builder = flatbuffers::FlatBufferBuilder::new();
    let series_root =
        details::build_enhanced_series_details(&mut series_builder, &series);
    series_builder.finish(series_root, None);
    let series = flatbuffers::root::<fb::details::EnhancedSeriesDetails>(
        series_builder.finished_data(),
    )
    .expect("series details root");
    assert_eq!(
        series.cast().expect("series cast").get(0).name(),
        Some("Actor One")
    );
    assert_eq!(
        series
            .recommendations()
            .expect("series recommendations")
            .get(0)
            .title(),
        Some("Recommended Title")
    );

    let mut episode = make_episode_details();
    episode.guest_stars = vec![make_cast_credit(
        "Guest Star",
        "Guest",
        Uuid::now_v7(),
        guest_profile_iid,
    )];
    episode.crew = vec![make_crew_credit(
        "Writer One",
        "Writer",
        "Writing",
        Uuid::now_v7(),
        crew_profile_iid,
    )];
    let mut episode_builder = flatbuffers::FlatBufferBuilder::new();
    let episode_root =
        details::build_episode_details(&mut episode_builder, &episode);
    episode_builder.finish(episode_root, None);
    let episode = flatbuffers::root::<fb::details::EpisodeDetails>(
        episode_builder.finished_data(),
    )
    .expect("episode details root");
    let guest = episode.guest_stars().expect("guest stars").get(0);
    assert_eq!(guest.name(), Some("Guest Star"));
    assert_eq!(
        fb_to_uuid(
            guest
                .profile()
                .expect("guest profile")
                .profile_iid()
                .expect("guest profile iid"),
        ),
        guest_profile_iid
    );
    assert_eq!(
        episode.crew().expect("episode crew").get(0).job(),
        Some("Writer")
    );
}

#[test]
fn series_bundle_data_packs_series_seasons_and_episodes() {
    let (series, seasons, episodes) = make_series_bundle();
    let bytes =
        batch_data::serialize_series_bundle_data(&batch_data::SeriesBundle {
            version: 22,
            series: &series,
            seasons: &seasons,
            episodes: &episodes,
        });
    let bundle = flatbuffers::root::<fb::library::SeriesBundleData>(&bytes)
        .expect("series bundle root");
    assert_eq!(fb_to_uuid(bundle.series_id()), *series.id.as_uuid());
    assert_eq!(bundle.version(), 22);

    let items = bundle.items().expect("items");
    assert_eq!(items.len(), 3);
    assert_eq!(
        items
            .get(0)
            .variant_as_series_reference()
            .expect("series variant")
            .title(),
        "Test Series"
    );
    assert_eq!(
        items
            .get(1)
            .variant_as_season_reference()
            .expect("season variant")
            .season_number(),
        1
    );
    assert_eq!(
        items
            .get(2)
            .variant_as_episode_reference()
            .expect("episode variant")
            .details()
            .expect("episode details")
            .name(),
        Some("Pilot")
    );
}
