use core::fmt;
use core::marker::PhantomData;
use core::ops::{Add, Sub};
use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};

const NANOS_PER_SECOND: i128 = 1_000_000_000;

/// Minimal chrono-like API so the crate can compile without the external dependency.
/// Intended for internal use when the `chrono` feature is disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utc;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime<Tz> {
    seconds: i64,
    nanos: u32,
    _tz: PhantomData<Tz>,
}

impl<Tz> fmt::Debug for DateTime<Tz> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "DateTime {{ seconds: {}, nanos: {} }}",
            self.seconds, self.nanos
        )
    }
}

impl<Tz> DateTime<Tz> {
    pub fn from_timestamp(seconds: i64, nanos: u32) -> Option<Self> {
        if nanos >= 1_000_000_000 {
            return None;
        }
        Some(Self {
            seconds,
            nanos,
            _tz: PhantomData,
        })
    }

    pub fn timestamp(&self) -> i64 {
        self.seconds
    }

    pub fn timestamp_subsec_nanos(&self) -> u32 {
        self.nanos
    }
}

impl DateTime<Utc> {
    pub fn signed_duration_since(&self, earlier: Self) -> Duration {
        let lhs = self.total_nanos();
        let rhs = earlier.total_nanos();
        Duration { nanos: lhs - rhs }
    }

    fn total_nanos(&self) -> i128 {
        (self.seconds as i128) * NANOS_PER_SECOND + self.nanos as i128
    }
}

impl Default for DateTime<Utc> {
    fn default() -> Self {
        Utc::now()
    }
}

impl Utc {
    pub fn now() -> DateTime<Utc> {
        SystemTime::now().into()
    }
}

impl From<SystemTime> for DateTime<Utc> {
    fn from(value: SystemTime) -> Self {
        let nanos = match value.duration_since(UNIX_EPOCH) {
            Ok(duration) => {
                (duration.as_secs() as i128) * NANOS_PER_SECOND
                    + duration.subsec_nanos() as i128
            }
            Err(err) => {
                let duration = err.duration();
                -((duration.as_secs() as i128) * NANOS_PER_SECOND
                    + duration.subsec_nanos() as i128)
            }
        };
        let seconds = nanos.div_euclid(NANOS_PER_SECOND);
        let remainder = nanos.rem_euclid(NANOS_PER_SECOND);
        DateTime::from_timestamp(seconds as i64, remainder as u32).unwrap()
    }
}

impl From<DateTime<Utc>> for SystemTime {
    fn from(value: DateTime<Utc>) -> Self {
        if value.seconds >= 0 {
            UNIX_EPOCH + StdDuration::new(value.seconds as u64, value.nanos)
        } else {
            let nanos =
                value.seconds as i128 * NANOS_PER_SECOND + value.nanos as i128;
            let positive = -nanos;
            let secs = positive / NANOS_PER_SECOND;
            let sub_nanos = positive % NANOS_PER_SECOND;
            UNIX_EPOCH - StdDuration::new(secs as u64, sub_nanos as u32)
        }
    }
}

impl Add<Duration> for DateTime<Utc> {
    type Output = Option<Self>;

    fn add(self, rhs: Duration) -> Self::Output {
        rhs.add_to(self)
    }
}

impl Sub<Duration> for DateTime<Utc> {
    type Output = Option<Self>;

    fn sub(self, rhs: Duration) -> Self::Output {
        rhs.negate().add_to(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Duration {
    nanos: i128,
}

impl Duration {
    pub fn from_std(duration: StdDuration) -> Self {
        Self {
            nanos: (duration.as_secs() as i128) * NANOS_PER_SECOND
                + duration.subsec_nanos() as i128,
        }
    }

    pub fn num_minutes(&self) -> i64 {
        (self.nanos / (60 * NANOS_PER_SECOND)) as i64
    }

    pub fn num_seconds(&self) -> i64 {
        (self.nanos / NANOS_PER_SECOND) as i64
    }

    fn add_to(self, datetime: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let total = datetime.total_nanos().checked_add(self.nanos)?;
        let seconds = total / NANOS_PER_SECOND;
        let nanos = (total % NANOS_PER_SECOND) as u32;
        DateTime::from_timestamp(seconds as i64, nanos)
    }

    fn negate(self) -> Self {
        Self { nanos: -self.nanos }
    }
}
