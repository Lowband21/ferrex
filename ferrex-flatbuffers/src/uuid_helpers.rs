//! Helpers for converting between `uuid::Uuid` and the FlatBuffers
//! `ferrex.ids.Uuid` struct.

use crate::fb::ids::Uuid as FbUuid;

/// Convert a `uuid::Uuid` to the FlatBuffers fixed-size representation.
#[inline]
pub fn uuid_to_fb(id: &uuid::Uuid) -> FbUuid {
    let b = id.as_bytes();
    FbUuid::new(
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10],
        b[11], b[12], b[13], b[14], b[15],
    )
}

/// Convert a FlatBuffers UUID struct back to `uuid::Uuid`.
#[inline]
pub fn fb_to_uuid(fb: &FbUuid) -> uuid::Uuid {
    uuid::Uuid::from_bytes([
        fb.b0(),
        fb.b1(),
        fb.b2(),
        fb.b3(),
        fb.b4(),
        fb.b5(),
        fb.b6(),
        fb.b7(),
        fb.b8(),
        fb.b9(),
        fb.b10(),
        fb.b11(),
        fb.b12(),
        fb.b13(),
        fb.b14(),
        fb.b15(),
    ])
}

/// All-zero UUID sentinel used when a nullable UUID field is absent.
#[inline]
pub fn nil_uuid() -> FbUuid {
    FbUuid::new(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
}

/// Convert an optional UUID to FlatBuffers, using nil for `None`.
#[inline]
pub fn option_uuid_to_fb(id: Option<&uuid::Uuid>) -> FbUuid {
    id.map_or_else(nil_uuid, uuid_to_fb)
}

/// Convert a FlatBuffers UUID to an optional UUID, treating nil as `None`.
#[inline]
pub fn fb_to_option_uuid(fb: &FbUuid) -> Option<uuid::Uuid> {
    let id = fb_to_uuid(fb);
    (!id.is_nil()).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_round_trips() {
        let original = uuid::Uuid::new_v4();
        let fb = uuid_to_fb(&original);
        assert_eq!(fb_to_uuid(&fb), original);
    }

    #[test]
    fn optional_uuid_uses_nil_sentinel() {
        let fb = option_uuid_to_fb(None);
        assert_eq!(fb_to_option_uuid(&fb), None);

        let id = uuid::Uuid::new_v4();
        let fb = option_uuid_to_fb(Some(&id));
        assert_eq!(fb_to_option_uuid(&fb), Some(id));
    }
}
