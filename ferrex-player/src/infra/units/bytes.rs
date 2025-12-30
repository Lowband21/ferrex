use std::fmt;

/// A strongly-typed byte size.
///
/// This is intentionally base-2 (KiB, MiB, GiB) because that's how we reason
/// about memory and most OS-level tools report it.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteSize(u64);

impl ByteSize {
    pub const ZERO: Self = Self(0);
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

    pub const fn from_bytes(bytes: u64) -> Self {
        Self(bytes)
    }

    pub fn from_usize(bytes: usize) -> Self {
        Self(u64::try_from(bytes).unwrap_or(u64::MAX))
    }

    pub const fn from_kib(kib: u64) -> Self {
        Self(kib.saturating_mul(Self::KIB as u64))
    }

    pub const fn from_mib(mib: u64) -> Self {
        Self(mib.saturating_mul(Self::MIB as u64))
    }

    pub const fn from_gib(gib: u64) -> Self {
        Self(gib.saturating_mul(Self::GIB as u64))
    }

    pub const fn as_bytes(self) -> u64 {
        self.0
    }

    pub const fn as_kib(self) -> f64 {
        self.0 as f64 / Self::KIB
    }

    pub const fn as_mib(self) -> f64 {
        self.0 as f64 / Self::MIB
    }

    pub const fn as_gib(self) -> f64 {
        self.0 as f64 / Self::GIB
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub fn as_mib_floor(self) -> u64 {
        self.0 / (1024 * 1024)
    }

    pub fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    pub fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    pub fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }
}

impl fmt::Debug for ByteSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} bytes", self.0)
    }
}

impl fmt::Display for ByteSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.0 as f64;
        if bytes >= Self::GIB {
            write!(f, "{:.2} GiB", bytes / Self::GIB)
        } else if bytes >= Self::MIB {
            write!(f, "{:.1} MiB", bytes / Self::MIB)
        } else if bytes >= Self::KIB {
            write!(f, "{:.1} KiB", bytes / Self::KIB)
        } else {
            write!(f, "{} B", self.0)
        }
    }
}
