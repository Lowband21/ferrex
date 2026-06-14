//! Unit helpers shared by player crates.

use std::fmt;

/// A strongly-typed byte size.
///
/// This is intentionally base-2 (KiB, MiB, GiB) because that is how Ferrex
/// reasons about memory budgets and how most OS-level tools report them.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteSize(u64);

impl ByteSize {
    /// Zero bytes.
    pub const ZERO: Self = Self(0);
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

    /// Construct from raw bytes.
    pub const fn from_bytes(bytes: u64) -> Self {
        Self(bytes)
    }

    /// Construct from a `usize`, saturating to `u64::MAX` on overflow.
    pub fn from_usize(bytes: usize) -> Self {
        Self(u64::try_from(bytes).unwrap_or(u64::MAX))
    }

    /// Construct from kibibytes.
    pub const fn from_kib(kib: u64) -> Self {
        Self(kib.saturating_mul(Self::KIB as u64))
    }

    /// Construct from mebibytes.
    pub const fn from_mib(mib: u64) -> Self {
        Self(mib.saturating_mul(Self::MIB as u64))
    }

    /// Construct from gibibytes.
    pub const fn from_gib(gib: u64) -> Self {
        Self(gib.saturating_mul(Self::GIB as u64))
    }

    /// Return raw bytes.
    pub const fn as_bytes(self) -> u64 {
        self.0
    }

    /// Return kibibytes as a floating-point value.
    pub const fn as_kib(self) -> f64 {
        self.0 as f64 / Self::KIB
    }

    /// Return mebibytes as a floating-point value.
    pub const fn as_mib(self) -> f64 {
        self.0 as f64 / Self::MIB
    }

    /// Return gibibytes as a floating-point value.
    pub const fn as_gib(self) -> f64 {
        self.0 as f64 / Self::GIB
    }

    /// Whether this represents zero bytes.
    pub fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Return the whole mebibytes contained in this byte size.
    pub fn as_mib_floor(self) -> u64 {
        self.0 / (1024 * 1024)
    }

    /// Saturating addition.
    pub fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    /// Saturating subtraction.
    pub fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    /// Return the larger of two byte sizes.
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

#[cfg(test)]
mod tests {
    use super::ByteSize;

    #[test]
    fn formats_base_two_units() {
        assert_eq!(ByteSize::from_bytes(42).to_string(), "42 B");
        assert_eq!(ByteSize::from_kib(2).to_string(), "2.0 KiB");
        assert_eq!(ByteSize::from_mib(3).to_string(), "3.0 MiB");
        assert_eq!(ByteSize::from_gib(4).to_string(), "4.00 GiB");
    }
}
