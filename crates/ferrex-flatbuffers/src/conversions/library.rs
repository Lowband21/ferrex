//! Conversions for library metadata payloads.

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use crate::conversions::common::{
    library_type_to_fb, option_timestamp_to_fb, timestamp_to_fb,
};
use crate::conversions::media::build_media;
use crate::fb::common::Timestamp;
use crate::fb::library as fb;
use crate::uuid_helpers::uuid_to_fb;

/// Build a FlatBuffers `Library` table from a full Ferrex library model.
pub fn build_library<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    library: &ferrex_model::Library,
) -> WIPOffset<fb::Library<'a>> {
    let id = uuid_to_fb(library.id.as_uuid());
    let name = builder.create_string(&library.name);
    let paths: Vec<_> = library
        .paths
        .iter()
        .map(|path| {
            let path = path.to_string_lossy();
            builder.create_string(path.as_ref())
        })
        .collect();
    let paths = (!paths.is_empty()).then(|| builder.create_vector(&paths));
    let last_scan = option_timestamp_to_fb(library.last_scan.as_ref());
    let created_at = timestamp_to_fb(&library.created_at);
    let updated_at = timestamp_to_fb(&library.updated_at);
    let media = match library.media.as_deref() {
        Some(items) if !items.is_empty() => {
            let media: Vec<_> = items
                .iter()
                .map(|item| build_media(builder, item))
                .collect();
            Some(builder.create_vector(&media))
        }
        _ => None,
    };

    fb::Library::create(
        builder,
        &fb::LibraryArgs {
            id: Some(&id),
            name: Some(name),
            library_type: library_type_to_fb(library.library_type),
            paths,
            scan_interval_minutes: library.scan_interval_minutes,
            last_scan: Some(&last_scan),
            enabled: library.enabled,
            auto_scan: library.auto_scan,
            watch_for_changes: library.watch_for_changes,
            analyze_on_scan: library.analyze_on_scan,
            max_retry_attempts: library.max_retry_attempts,
            movie_ref_batch_size: library.movie_ref_batch_size.get(),
            created_at: Some(&created_at),
            updated_at: Some(&updated_at),
            media,
        },
    )
}

/// Build a FlatBuffers `Library` table from a lightweight library reference.
pub fn build_library_reference<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    library: &ferrex_model::details::LibraryReference,
) -> WIPOffset<fb::Library<'a>> {
    let id = uuid_to_fb(library.id.as_uuid());
    let name = builder.create_string(&library.name);
    let paths: Vec<_> = library
        .paths
        .iter()
        .map(|path| {
            let path = path.to_string_lossy();
            builder.create_string(path.as_ref())
        })
        .collect();
    let paths = (!paths.is_empty()).then(|| builder.create_vector(&paths));
    let zero = Timestamp::new(0);

    fb::Library::create(
        builder,
        &fb::LibraryArgs {
            id: Some(&id),
            name: Some(name),
            library_type: library_type_to_fb(library.library_type),
            paths,
            scan_interval_minutes: 0,
            last_scan: Some(&zero),
            enabled: true,
            auto_scan: false,
            watch_for_changes: false,
            analyze_on_scan: false,
            max_retry_attempts: 0,
            movie_ref_batch_size: 0,
            created_at: Some(&zero),
            updated_at: Some(&zero),
            media: None,
        },
    )
}

/// Serialize full library models into a complete `LibraryList` buffer.
pub fn serialize_library_list(libraries: &[ferrex_model::Library]) -> Vec<u8> {
    let mut builder =
        FlatBufferBuilder::with_capacity(1024 * libraries.len().max(1));
    let items: Vec<_> = libraries
        .iter()
        .map(|library| build_library(&mut builder, library))
        .collect();
    finish_library_list(builder, &items)
}

/// Serialize lightweight library references into a complete `LibraryList` buffer.
pub fn serialize_library_reference_list(
    libraries: &[ferrex_model::details::LibraryReference],
) -> Vec<u8> {
    let mut builder =
        FlatBufferBuilder::with_capacity(512 * libraries.len().max(1));
    let items: Vec<_> = libraries
        .iter()
        .map(|library| build_library_reference(&mut builder, library))
        .collect();
    finish_library_list(builder, &items)
}

fn finish_library_list<'a>(
    mut builder: FlatBufferBuilder<'a>,
    items: &[WIPOffset<fb::Library<'a>>],
) -> Vec<u8> {
    let items = builder.create_vector(items);
    let list = fb::LibraryList::create(
        &mut builder,
        &fb::LibraryListArgs { items: Some(items) },
    );
    builder.finish(list, None);
    builder.finished_data().to_vec()
}
