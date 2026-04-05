//! Conversions for auth/setup types → FlatBuffers.

use flatbuffers::FlatBufferBuilder;

use crate::fb::auth as fb;
use crate::uuid_helpers::option_uuid_to_fb;

/// Serialize a `SetupStatus` into a complete FlatBuffers buffer.
///
/// The FlatBuffers `SetupStatus` schema is minimal (needs_setup, registration_open)
/// compared to the server's richer JSON `SetupStatus`. This is intentional —
/// mobile clients only need to know whether to show the setup flow.
pub fn serialize_setup_status(needs_setup: bool, registration_open: bool) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::with_capacity(64);

    let status = fb::SetupStatus::create(&mut builder, &fb::SetupStatusArgs {
        needs_setup,
        registration_open,
    });

    builder.finish(status, None);
    builder.finished_data().to_vec()
}

/// Serialize an `AuthToken` into a complete FlatBuffers buffer.
///
/// Maps from the server's `ferrex_core::domain::users::user::AuthToken` which
/// has additional fields (device_session_id, user_id, scope) that aren't in
/// the FlatBuffers schema — mobile clients don't need them.
pub fn serialize_auth_token(
    access_token: &str,
    refresh_token: &str,
    expires_in: u32,
    session_id: Option<&uuid::Uuid>,
) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::with_capacity(256);

    let access = builder.create_string(access_token);
    let refresh = builder.create_string(refresh_token);
    let sid = session_id.map(|id| option_uuid_to_fb(Some(id)));

    let token = fb::AuthToken::create(&mut builder, &fb::AuthTokenArgs {
        access_token: Some(access),
        refresh_token: Some(refresh),
        expires_in,
        session_id: sid.as_ref(),
    });

    builder.finish(token, None);
    builder.finished_data().to_vec()
}
