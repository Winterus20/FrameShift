use crate::error::TimeError;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

/// Number of Flicks per second (`705,600,000`).
///
/// Evenly divisible by standard frame rates (`24, 25, 30, 48, 50, 60, 120` fps),
/// NTSC `1000/1001` rates (`23.976, 29.97, 59.94` fps), and standard audio sample
/// rates (`44.1 kHz, 48 kHz, 96 kHz`).
pub const FLICKS_PER_SECOND: i64 = 705_600_000;

/// Number of Flicks per millisecond (`705,600`).
pub const FLICKS_PER_MILLISECOND: i64 = 705_600;

/// Persistent exact time unit in FrameShift (`1 / 705,600,000` second).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[repr(transparent)]
pub struct Flicks(pub i64);

impl Flicks {
    /// Zero ticks.
    pub const ZERO: Self = Self(0);

    /// Minimum representable tick value.
    pub const MIN: Self = Self(i64::MIN);

    /// Maximum representable tick value.
    pub const MAX: Self = Self(i64::MAX);

    /// Creates a `Flicks` value from raw ticks.
    #[inline]
    pub const fn from_raw(ticks: i64) -> Self {
        Self(ticks)
    }

    /// Returns the underlying `i64` tick count.
    #[inline]
    pub const fn as_i64(self) -> i64 {
        self.0
    }

    /// Converts an exact integer number of seconds into `Flicks`.
    #[inline]
    pub fn from_seconds_i64(seconds: i64) -> Result<Self, TimeError> {
        seconds
            .checked_mul(FLICKS_PER_SECOND)
            .map(Self)
            .ok_or(TimeError::Overflow)
    }

    /// Converts an exact integer number of milliseconds into `Flicks`.
    #[inline]
    pub fn from_millis_i64(millis: i64) -> Result<Self, TimeError> {
        millis
            .checked_mul(FLICKS_PER_MILLISECOND)
            .map(Self)
            .ok_or(TimeError::Overflow)
    }

    /// Converts a floating-point second value into `Flicks` by rounding to nearest tick.
    ///
    /// Intended only for UI/display boundaries or intermediate evaluation, never as
    /// persistent project source of truth.
    #[inline]
    pub fn from_seconds_f64_approx(seconds: f64) -> Result<Self, TimeError> {
        if !seconds.is_finite() {
            return Err(TimeError::Overflow);
        }
        let scaled = (seconds * FLICKS_PER_SECOND as f64).round();
        if scaled < i64::MIN as f64 || scaled > i64::MAX as f64 {
            return Err(TimeError::Overflow);
        }
        Ok(Self(scaled as i64))
    }

    /// Converts `Flicks` to `f64` seconds for display or intermediate shader/curve evaluation.
    #[inline]
    pub fn to_seconds_f64(self) -> f64 {
        self.0 as f64 / FLICKS_PER_SECOND as f64
    }

    /// Checked addition.
    #[inline]
    pub fn checked_add(self, rhs: Self) -> Result<Self, TimeError> {
        self.0
            .checked_add(rhs.0)
            .map(Self)
            .ok_or(TimeError::Overflow)
    }

    /// Checked subtraction.
    #[inline]
    pub fn checked_sub(self, rhs: Self) -> Result<Self, TimeError> {
        self.0
            .checked_sub(rhs.0)
            .map(Self)
            .ok_or(TimeError::Overflow)
    }

    /// Checked integer multiplication.
    #[inline]
    pub fn checked_mul(self, factor: i64) -> Result<Self, TimeError> {
        self.0
            .checked_mul(factor)
            .map(Self)
            .ok_or(TimeError::Overflow)
    }

    /// Checked integer division.
    #[inline]
    pub fn checked_div(self, divisor: i64) -> Result<Self, TimeError> {
        self.0
            .checked_div(divisor)
            .map(Self)
            .ok_or(TimeError::Overflow)
    }

    /// Saturating addition.
    #[inline]
    pub const fn saturating_add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }

    /// Saturating subtraction.
    #[inline]
    pub const fn saturating_sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }

    /// Checked absolute value.
    #[inline]
    pub fn checked_abs(self) -> Result<Self, TimeError> {
        self.0.checked_abs().map(Self).ok_or(TimeError::Overflow)
    }

    /// Clamps `self` between `min` and `max`.
    #[inline]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self(self.0.clamp(min.0, max.0))
    }
}

impl fmt::Display for Flicks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} flicks ({:.6}s)", self.0, self.to_seconds_f64())
    }
}

impl Add for Flicks {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub for Flicks {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl Neg for Flicks {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

impl AddAssign for Flicks {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl SubAssign for Flicks {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}
