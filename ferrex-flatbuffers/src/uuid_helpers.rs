//! Helpers for converting between `uuid::Uuid` and the FlatBuffers
//! `ferrex.ids.Uuid` struct (16 raw bytes).

use crate::fb::ids::Uuid as FbUuid;

/// Convert a `uuid::Uuid` to the FlatBuffers struct representation.
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

/// Sentinel UUID for representing `None` optional UUIDs in FlatBuffers structs.
/// All zeros — callers should treat this as "absent".
pub fn nil_uuid() -> FbUuid {
    FbUuid::new(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
}

/// Convert an `Option<uuid::Uuid>` to FlatBuffers, using NIL for None.
#[inline]
pub fn option_uuid_to_fb(id: Option<&uuid::Uuid>) -> FbUuid {
    match id {
        Some(id) => uuid_to_fb(id),
        None => nil_uuid(),
    }
}

/// Convert a FlatBuffers UUID to `Option<uuid::Uuid>`, treating nil as None.
#[inline]
pub fn fb_to_option_uuid(fb: &FbUuid) -> Option<uuid::Uuid> {
    let id = fb_to_uuid(fb);
    if id.is_nil() { None } else { Some(id) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_uuid() {
        let original = uuid::Uuid::new_v4();
        let fb = uuid_to_fb(&original);
        let back = fb_to_uuid(&fb);
        assert_eq!(original, back);
    }

    #[test]
    fn nil_uuid_round_trip() {
        let nil = uuid::Uuid::nil();
        let fb = uuid_to_fb(&nil);
        let back = fb_to_option_uuid(&fb);
        assert_eq!(back, None);
    }

    #[test]
    fn some_uuid_round_trip() {
        let original = uuid::Uuid::new_v4();
        let fb = option_uuid_to_fb(Some(&original));
        let back = fb_to_option_uuid(&fb);
        assert_eq!(back, Some(original));
    }

    #[test]
    fn none_uuid_round_trip() {
        let fb = option_uuid_to_fb(None);
        let back = fb_to_option_uuid(&fb);
        assert_eq!(back, None);
    }
}
