//! Conversions for `ferrex_model::files` types.

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use crate::conversions::common::timestamp_to_fb;
use crate::fb::common::VideoMediaType;
use crate::fb::files as fb;
use crate::uuid_helpers::uuid_to_fb;

fn string_opt<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    value: Option<&str>,
) -> Option<flatbuffers::WIPOffset<&'a str>> {
    value.map(|value| builder.create_string(value))
}

fn media_id_parts(
    media_id: &ferrex_model::MediaID,
) -> (VideoMediaType, uuid::Uuid) {
    match media_id {
        ferrex_model::MediaID::Movie(id) => {
            (VideoMediaType::Movie, id.to_uuid())
        }
        ferrex_model::MediaID::Series(id) => {
            (VideoMediaType::Series, id.to_uuid())
        }
        ferrex_model::MediaID::Season(id) => {
            (VideoMediaType::Season, id.to_uuid())
        }
        ferrex_model::MediaID::Episode(id) => {
            (VideoMediaType::Episode, id.to_uuid())
        }
    }
}

/// Build a FlatBuffers `MediaFile` table.
pub fn build_media_file<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    file: &ferrex_model::MediaFile,
) -> WIPOffset<fb::MediaFile<'a>> {
    let id = uuid_to_fb(&file.id);
    let (media_id_type, media_id) = media_id_parts(&file.media_id);
    let media_id_uuid = uuid_to_fb(&media_id);
    let path = file.path.to_string_lossy();
    let path = builder.create_string(path.as_ref());
    let filename = builder.create_string(&file.filename);
    let discovered_at = timestamp_to_fb(&file.discovered_at);
    let created_at = timestamp_to_fb(&file.created_at);
    let library_id = uuid_to_fb(file.library_id.as_uuid());
    let metadata = file
        .media_file_metadata
        .as_ref()
        .map(|metadata| build_media_file_metadata(builder, metadata));

    fb::MediaFile::create(
        builder,
        &fb::MediaFileArgs {
            id: Some(&id),
            media_id_type,
            media_id_uuid: Some(&media_id_uuid),
            path: Some(path),
            filename: Some(filename),
            size: file.size,
            discovered_at: Some(&discovered_at),
            created_at: Some(&created_at),
            metadata,
            library_id: Some(&library_id),
        },
    )
}

/// Build a FlatBuffers `MediaFileMetadata` table.
pub fn build_media_file_metadata<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    metadata: &ferrex_model::MediaFileMetadata,
) -> WIPOffset<fb::MediaFileMetadata<'a>> {
    let video_codec = string_opt(builder, metadata.video_codec.as_deref());
    let audio_codec = string_opt(builder, metadata.audio_codec.as_deref());
    let color_primaries =
        string_opt(builder, metadata.color_primaries.as_deref());
    let color_transfer =
        string_opt(builder, metadata.color_transfer.as_deref());
    let color_space = string_opt(builder, metadata.color_space.as_deref());
    let parsed_info = metadata
        .parsed_info
        .as_ref()
        .map(|parsed| build_parsed_media_info(builder, parsed));

    fb::MediaFileMetadata::create(
        builder,
        &fb::MediaFileMetadataArgs {
            duration: metadata.duration.unwrap_or(0.0),
            width: metadata.width.unwrap_or(0),
            height: metadata.height.unwrap_or(0),
            video_codec,
            audio_codec,
            bitrate: metadata.bitrate.unwrap_or(0),
            framerate: metadata.framerate.unwrap_or(0.0),
            file_size: metadata.file_size,
            color_primaries,
            color_transfer,
            color_space,
            bit_depth: metadata.bit_depth.unwrap_or(0),
            parsed_info,
        },
    )
}

/// Build a FlatBuffers `ParsedMediaInfo` union wrapper.
pub fn build_parsed_media_info<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    parsed: &ferrex_model::files::ParsedMediaInfo,
) -> WIPOffset<fb::ParsedMediaInfo<'a>> {
    match parsed {
        ferrex_model::files::ParsedMediaInfo::Movie(movie) => {
            let movie = build_parsed_movie_info(builder, movie);
            fb::ParsedMediaInfo::create(
                builder,
                &fb::ParsedMediaInfoArgs {
                    variant_type: fb::ParsedMediaInfoVariant::ParsedMovieInfo,
                    variant: Some(movie.as_union_value()),
                },
            )
        }
        ferrex_model::files::ParsedMediaInfo::Episode(episode) => {
            let episode = build_parsed_episode_info(builder, episode);
            fb::ParsedMediaInfo::create(
                builder,
                &fb::ParsedMediaInfoArgs {
                    variant_type: fb::ParsedMediaInfoVariant::ParsedEpisodeInfo,
                    variant: Some(episode.as_union_value()),
                },
            )
        }
    }
}

fn build_parsed_movie_info<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    movie: &ferrex_model::files::ParsedMovieInfo,
) -> WIPOffset<fb::ParsedMovieInfo<'a>> {
    let title = builder.create_string(&movie.title);
    let resolution = string_opt(builder, movie.resolution.as_deref());
    let source = string_opt(builder, movie.source.as_deref());
    let release_group = string_opt(builder, movie.release_group.as_deref());

    fb::ParsedMovieInfo::create(
        builder,
        &fb::ParsedMovieInfoArgs {
            title: Some(title),
            year: movie.year.unwrap_or(0),
            resolution,
            source,
            release_group,
        },
    )
}

fn build_parsed_episode_info<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    episode: &ferrex_model::files::ParsedEpisodeInfo,
) -> WIPOffset<fb::ParsedEpisodeInfo<'a>> {
    let show_name = builder.create_string(&episode.show_name);
    let episode_title = string_opt(builder, episode.episode_title.as_deref());
    let resolution = string_opt(builder, episode.resolution.as_deref());
    let source = string_opt(builder, episode.source.as_deref());
    let release_group = string_opt(builder, episode.release_group.as_deref());

    fb::ParsedEpisodeInfo::create(
        builder,
        &fb::ParsedEpisodeInfoArgs {
            show_name: Some(show_name),
            season: episode.season,
            episode: episode.episode,
            episode_title,
            year: episode.year.unwrap_or(0),
            resolution,
            source,
            release_group,
        },
    )
}
