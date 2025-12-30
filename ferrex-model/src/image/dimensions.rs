use std::num::NonZeroU32;

/// Non-zero pixel dimensions for a decoded image.
///
/// This is intentionally independent of `ImageSize`, because `ImageSize` is a
/// *requested* or *logical* size (e.g. "original", "w780"), while decoded
/// dimensions are the authoritative width/height of the actual bytes stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
#[cfg_attr(feature = "rkyv", rkyv(derive(Debug, PartialEq, Eq, Hash)))]
pub struct ImageDimensions {
    pub width: NonZeroU32,
    pub height: NonZeroU32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageDimensionsError {
    ZeroWidth,
    ZeroHeight,
}

impl ImageDimensions {
    pub const fn new(width: NonZeroU32, height: NonZeroU32) -> Self {
        Self { width, height }
    }

    pub const fn width_u32(self) -> u32 {
        self.width.get()
    }

    pub const fn height_u32(self) -> u32 {
        self.height.get()
    }

    pub const fn as_u32_tuple(self) -> (u32, u32) {
        (self.width.get(), self.height.get())
    }
}

impl TryFrom<(u32, u32)> for ImageDimensions {
    type Error = ImageDimensionsError;

    fn try_from(value: (u32, u32)) -> Result<Self, Self::Error> {
        let (width, height) = value;
        let width =
            NonZeroU32::new(width).ok_or(ImageDimensionsError::ZeroWidth)?;
        let height =
            NonZeroU32::new(height).ok_or(ImageDimensionsError::ZeroHeight)?;
        Ok(Self { width, height })
    }
}
