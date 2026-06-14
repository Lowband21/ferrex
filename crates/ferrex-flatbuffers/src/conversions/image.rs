//! Conversions for mobile image manifest/readiness payloads.

use flatbuffers::{FlatBufferBuilder, WIPOffset};
use uuid::Uuid;

use crate::fb::{common as common_fb, image as image_fb};
use crate::uuid_helpers::{fb_to_uuid, uuid_to_fb};

/// Borrowed image manifest query decoded from FlatBuffers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageManifestQuery {
    pub iid: Uuid,
    pub category: common_fb::ImageCategory,
}

/// Status carried by the FlatBuffers image manifest contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageManifestEntryStatus<'a> {
    Ready { token: &'a str },
    Pending { retry_after_ms: u64 },
    Failed { reason: Option<&'a str> },
}

/// Borrowed image manifest entry encoded as FlatBuffers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageManifestEntry<'a> {
    pub iid: Uuid,
    pub category: common_fb::ImageCategory,
    pub status: ImageManifestEntryStatus<'a>,
}

/// Parse an image manifest request.
pub fn parse_image_manifest_request(
    bytes: &[u8],
) -> Result<Vec<ImageManifestQuery>, flatbuffers::InvalidFlatbuffer> {
    let request = flatbuffers::root::<image_fb::ImageManifestRequest>(bytes)?;
    let queries = request
        .queries()
        .map(|items| {
            items
                .iter()
                .map(|query| ImageManifestQuery {
                    iid: fb_to_uuid(query.iid()),
                    category: query.category(),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(queries)
}

fn build_image_manifest_entry<'a>(
    builder: &mut FlatBufferBuilder<'a>,
    entry: &ImageManifestEntry<'_>,
) -> WIPOffset<image_fb::ImageManifestEntry<'a>> {
    let iid = uuid_to_fb(&entry.iid);
    let (status, token, retry_after_millis, failure_reason) = match entry.status
    {
        ImageManifestEntryStatus::Ready { token } => (
            image_fb::ImageStatus::Ready,
            Some(builder.create_string(token)),
            0,
            None,
        ),
        ImageManifestEntryStatus::Pending { retry_after_ms } => {
            (image_fb::ImageStatus::Pending, None, retry_after_ms, None)
        }
        ImageManifestEntryStatus::Failed { reason } => (
            image_fb::ImageStatus::Failed,
            None,
            0,
            reason.map(|reason| builder.create_string(reason)),
        ),
    };

    image_fb::ImageManifestEntry::create(
        builder,
        &image_fb::ImageManifestEntryArgs {
            iid: Some(&iid),
            status,
            token,
            category: entry.category,
            retry_after_millis,
            failure_reason,
        },
    )
}

/// Serialize an image manifest response.
pub fn serialize_image_manifest_response(
    entries: &[ImageManifestEntry<'_>],
) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::with_capacity(64 + 48 * entries.len());
    let entries = entries
        .iter()
        .map(|entry| build_image_manifest_entry(&mut builder, entry))
        .collect::<Vec<_>>();
    let entries = builder.create_vector(&entries);
    let response = image_fb::ImageManifestResponse::create(
        &mut builder,
        &image_fb::ImageManifestResponseArgs {
            entries: Some(entries),
        },
    );
    builder.finish(response, None);
    builder.finished_data().to_vec()
}
