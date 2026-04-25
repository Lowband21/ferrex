//! Conversions for `ferrex_model::files` → FlatBuffers.

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use crate::conversions::common::timestamp_to_fb;
use crate::fb::common::VideoMediaType;
use crate::fb::files as fb;
use crate::uuid_helpers::uuid_to_fb;

use ferrex_model::MediaID;

/// Convert a `ferrex_model::MediaID` to the (enum, uuid) pair used in FlatBuffers.
fn media_id_parts(mid: &MediaID) -> (VideoMediaType, uuid::Uuid) {
    match mid {
        MediaID::Movie(id) => (VideoMediaType::Movie, id.to_uuid()),
        MediaID::Series(id) => (VideoMediaType::Series, id.to_uuid()),
        MediaID::Season(id) => (VideoMediaType::Season, id.to_uuid()),
        MediaID::Episode(id) => (VideoMediaType::Episode, id.to_uuid()),
    }
}

/// Build a `MediaFile` table.
pub fn build_media_file<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    f: &ferrex_model::MediaFile,
) -> WIPOffset<fb::MediaFile<'a>> {
    let id = uuid_to_fb(&f.id);
    let (media_type, media_uuid) = media_id_parts(&f.media_id);
    let media_uuid_fb = uuid_to_fb(&media_uuid);
    let path = builder.create_string(&f.path.to_string_lossy());
    let filename = builder.create_string(&f.filename);
    let discovered_at = timestamp_to_fb(&f.discovered_at);
    let created_at = timestamp_to_fb(&f.created_at);
    let library_id = uuid_to_fb(f.library_id.as_uuid());

    let metadata = f
        .media_file_metadata
        .as_ref()
        .map(|m| build_media_file_metadata(builder, m));

    fb::MediaFile::create(
        builder,
        &fb::MediaFileArgs {
            id: Some(&id),
            media_id_type: media_type,
            media_id_uuid: Some(&media_uuid_fb),
            path: Some(path),
            filename: Some(filename),
            size: f.size,
            discovered_at: Some(&discovered_at),
            created_at: Some(&created_at),
            metadata,
            library_id: Some(&library_id),
        },
    )
}

/// Build a `MediaFileMetadata` table.
fn build_media_file_metadata<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    m: &ferrex_model::MediaFileMetadata,
) -> WIPOffset<fb::MediaFileMetadata<'a>> {
    let video_codec =
        m.video_codec.as_deref().map(|s| builder.create_string(s));
    let audio_codec =
        m.audio_codec.as_deref().map(|s| builder.create_string(s));
    let color_primaries = m
        .color_primaries
        .as_deref()
        .map(|s| builder.create_string(s));
    let color_transfer = m
        .color_transfer
        .as_deref()
        .map(|s| builder.create_string(s));
    let color_space =
        m.color_space.as_deref().map(|s| builder.create_string(s));

    fb::MediaFileMetadata::create(
        builder,
        &fb::MediaFileMetadataArgs {
            duration: m.duration.unwrap_or(0.0),
            width: m.width.unwrap_or(0),
            height: m.height.unwrap_or(0),
            video_codec,
            audio_codec,
            bitrate: m.bitrate.unwrap_or(0),
            framerate: m.framerate.unwrap_or(0.0),
            file_size: m.file_size,
            color_primaries,
            color_transfer,
            color_space,
            bit_depth: m.bit_depth.unwrap_or(0),
            parsed_info: None, // TODO: implement if needed for mobile
        },
    )
}
