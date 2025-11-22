use std::path::PathBuf;

use ferrex_contracts::{
    id::MediaIDLike,
    media_ops::{MediaOps, Playable},
};
use ferrex_core::infrastructure::archive::ArchivedModel;
use ferrex_model::{
    LibraryId, MovieID,
    chrono::Timelike,
    details::MediaDetailsOption,
    files::{MediaFile, MediaFileMetadata},
    library::{ArchivedLibrary, ArchivedLibraryExt, Library, LibraryType},
    media::{ArchivedMedia, Media, MovieReference},
    titles::MovieTitle,
    urls::{MovieURL, UrlLike},
};
use rkyv::{rancor::Error as RkyvError, to_bytes};
use uuid::Uuid;

struct SampleMovie {
    library_id: LibraryId,
    media: Media,
}

fn truncated_now() -> ferrex_model::chrono::DateTime<ferrex_model::chrono::Utc>
{
    let now = ferrex_model::chrono::Utc::now();
    now.with_nanosecond(0)
        .expect("valid timestamp when dropping subsecond precision")
}

fn sample_movie() -> SampleMovie {
    let library_id = LibraryId::new();
    let movie_id = MovieID::new();
    let now = truncated_now();

    let media_file = MediaFile {
        id: Uuid::now_v7(),
        path: PathBuf::from("/library/movies/inception.mkv"),
        filename: "inception.mkv".to_string(),
        size: 1_073_741_824,
        discovered_at: now,
        created_at: now,
        media_file_metadata: Some(MediaFileMetadata {
            duration: Some(7_200.0),
            width: Some(1920),
            height: Some(1080),
            video_codec: Some("h264".into()),
            audio_codec: Some("dts".into()),
            bitrate: Some(8_000_000),
            framerate: Some(23.976),
            file_size: 1_073_741_824,
            color_primaries: Some("bt709".into()),
            color_transfer: Some("bt709".into()),
            color_space: Some("bt709".into()),
            bit_depth: Some(8),
            parsed_info: None,
        }),
        library_id,
    };

    let movie_ref = MovieReference {
        id: movie_id,
        library_id,
        tmdb_id: 27205,
        title: MovieTitle::from("Inception"),
        details: MediaDetailsOption::Endpoint("/tmdb/movies/27205".into()),
        endpoint: MovieURL::from_string("/movies/inception".into()),
        file: media_file,
        theme_color: Some("#0a0f24".into()),
    };

    SampleMovie {
        library_id,
        media: Media::Movie(movie_ref),
    }
}

#[test]
fn archived_media_round_trip_preserves_media_ops() {
    let original_sample = sample_movie();
    let original = original_sample.media.clone();
    let bytes = to_bytes::<RkyvError>(&original)
        .expect("serializing media snapshot should succeed");
    let archived = rkyv::access::<ArchivedMedia, RkyvError>(&bytes)
        .expect("accessing archived media snapshot should succeed");

    // Ensure the archived payload can be materialized back to the owned model.
    let restored = archived.to_model();
    assert_eq!(restored, original);

    // MediaOps contract parity between archived and owned variants.
    assert_eq!(MediaOps::media_id(&restored), MediaOps::media_id(&original));
    assert_eq!(
        MediaOps::media_id(archived).to_uuid(),
        MediaOps::media_id(&original).to_uuid()
    );
    assert_eq!(MediaOps::endpoint(archived), MediaOps::endpoint(&original));
    assert_eq!(
        MediaOps::theme_color(archived),
        MediaOps::theme_color(&original)
    );

    // Playable duration should survive round-trip (converted from metadata seconds).
    let restored_movie = match &restored {
        Media::Movie(movie) => movie,
        _ => panic!("expected movie media after round-trip"),
    };
    assert_eq!(
        Playable::duration(restored_movie)
            .map(|duration| duration.as_secs_f64()),
        Some(7_200.0)
    );
}

#[test]
fn archived_library_round_trip_retains_members() {
    let sample = sample_movie();
    let movie = sample.media.clone();
    let timestamp = truncated_now();
    let library = Library {
        id: sample.library_id,
        name: "Primary Movies".into(),
        library_type: LibraryType::Movies,
        paths: vec![PathBuf::from("/library/movies")],
        scan_interval_minutes: 60,
        last_scan: None,
        enabled: true,
        auto_scan: true,
        watch_for_changes: true,
        analyze_on_scan: false,
        max_retry_attempts: 3,
        created_at: timestamp,
        updated_at: timestamp,
        media: Some(vec![movie.clone()]),
    };

    let bytes = to_bytes::<RkyvError>(&library)
        .expect("serializing library snapshot should succeed");
    let archived = rkyv::access::<ArchivedLibrary, RkyvError>(&bytes)
        .expect("accessing archived library snapshot should succeed");

    // Library itself should deserialize cleanly.
    let restored = archived.to_model();
    assert_eq!(restored.id, library.id);
    assert_eq!(restored.name, library.name);
    assert_eq!(restored.library_type, library.library_type);
    assert_eq!(restored.paths, library.paths);
    assert_eq!(
        restored.scan_interval_minutes,
        library.scan_interval_minutes
    );
    assert_eq!(restored.last_scan, library.last_scan);
    assert_eq!(restored.enabled, library.enabled);
    assert_eq!(restored.auto_scan, library.auto_scan);
    assert_eq!(restored.watch_for_changes, library.watch_for_changes);
    assert_eq!(restored.analyze_on_scan, library.analyze_on_scan);
    assert_eq!(restored.max_retry_attempts, library.max_retry_attempts);
    assert_eq!(restored.created_at, library.created_at);
    assert_eq!(restored.updated_at, library.updated_at);
    assert_eq!(restored.media, library.media);

    // The archived extension helpers should expose the embedded media.
    let archived_media = archived
        .media_as_slice()
        .first()
        .expect("library should include serialized media");
    let hydrated_media = archived_media.to_model();
    assert_eq!(hydrated_media, movie);

    // Access nested movie references directly.
    let archived_movie_ref = archived
        .get_movie_refs()
        .next()
        .expect("movie library should expose archived movie references");
    let owned_movie_ref: MovieReference = archived_movie_ref.to_model();
    if let Media::Movie(expected) = movie {
        assert_eq!(owned_movie_ref, expected);
    } else {
        panic!("expected sample movie media");
    }
}
