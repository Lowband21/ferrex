//! Conversions for `ferrex_model::Library` → FlatBuffers.

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use crate::conversions::common::{
    library_type_to_fb, option_timestamp_to_fb, timestamp_to_fb,
};
use crate::fb::library as fb;
use crate::uuid_helpers::uuid_to_fb;

/// Serialize a `ferrex_model::Library` into a FlatBuffers `Library` table.
pub fn build_library<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    lib: &ferrex_model::Library,
) -> WIPOffset<fb::Library<'a>> {
    let id = uuid_to_fb(lib.id.as_uuid());
    let name = builder.create_string(&lib.name);

    let paths: Vec<_> = lib
        .paths
        .iter()
        .map(|p| builder.create_string(&p.to_string_lossy()))
        .collect();
    let paths = if paths.is_empty() {
        None
    } else {
        Some(builder.create_vector(&paths))
    };

    let last_scan = option_timestamp_to_fb(lib.last_scan.as_ref());
    let created_at = timestamp_to_fb(&lib.created_at);
    let updated_at = timestamp_to_fb(&lib.updated_at);

    fb::Library::create(
        builder,
        &fb::LibraryArgs {
            id: Some(&id),
            name: Some(name),
            library_type: library_type_to_fb(&lib.library_type),
            paths,
            scan_interval_minutes: lib.scan_interval_minutes,
            last_scan: Some(&last_scan),
            enabled: lib.enabled,
            auto_scan: lib.auto_scan,
            watch_for_changes: lib.watch_for_changes,
            analyze_on_scan: lib.analyze_on_scan,
            max_retry_attempts: lib.max_retry_attempts,
            movie_ref_batch_size: lib.movie_ref_batch_size.get(),
            created_at: Some(&created_at),
            updated_at: Some(&updated_at),
            media: None, // Media is fetched via batch sync, not embedded
        },
    )
}

/// Serialize a `ferrex_model::LibraryReference` into a FlatBuffers `Library` table.
/// LibraryReference is a lightweight subset — only id, name, type, paths.
pub fn build_library_reference<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    lib: &ferrex_model::details::LibraryReference,
) -> WIPOffset<fb::Library<'a>> {
    let id = uuid_to_fb(lib.id.as_uuid());
    let name = builder.create_string(&lib.name);

    let paths: Vec<_> = lib
        .paths
        .iter()
        .map(|p| builder.create_string(&p.to_string_lossy()))
        .collect();
    let paths = if paths.is_empty() {
        None
    } else {
        Some(builder.create_vector(&paths))
    };

    // Use zero-value defaults for fields not present in LibraryReference
    let zero_ts = crate::fb::common::Timestamp::new(0);

    fb::Library::create(
        builder,
        &fb::LibraryArgs {
            id: Some(&id),
            name: Some(name),
            library_type: library_type_to_fb(&lib.library_type),
            paths,
            scan_interval_minutes: 0,
            last_scan: Some(&zero_ts),
            enabled: true,
            auto_scan: false,
            watch_for_changes: false,
            analyze_on_scan: false,
            max_retry_attempts: 0,
            movie_ref_batch_size: 0,
            created_at: Some(&zero_ts),
            updated_at: Some(&zero_ts),
            media: None,
        },
    )
}

/// Serialize a `Vec<LibraryReference>` into a complete FlatBuffers `LibraryList` buffer.
pub fn serialize_library_reference_list(
    libraries: &[ferrex_model::details::LibraryReference],
) -> Vec<u8> {
    let mut builder =
        FlatBufferBuilder::with_capacity(512 * libraries.len().max(1));

    let items: Vec<_> = libraries
        .iter()
        .map(|lib| build_library_reference(&mut builder, lib))
        .collect();

    let items = builder.create_vector(&items);
    let list = fb::LibraryList::create(
        &mut builder,
        &fb::LibraryListArgs { items: Some(items) },
    );

    builder.finish(list, None);
    builder.finished_data().to_vec()
}

/// Serialize a `Vec<Library>` into a complete FlatBuffers `LibraryList` buffer.
/// Returns owned bytes ready to send as an HTTP response body.
pub fn serialize_library_list(libraries: &[ferrex_model::Library]) -> Vec<u8> {
    let mut builder =
        FlatBufferBuilder::with_capacity(1024 * libraries.len().max(1));

    let items: Vec<_> = libraries
        .iter()
        .map(|lib| build_library(&mut builder, lib))
        .collect();

    let items = builder.create_vector(&items);
    let list = fb::LibraryList::create(
        &mut builder,
        &fb::LibraryListArgs { items: Some(items) },
    );

    builder.finish(list, None);
    builder.finished_data().to_vec()
}
