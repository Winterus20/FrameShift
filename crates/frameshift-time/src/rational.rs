use crate::error::TimeError;
use crate::flicks::{Flicks, FLICKS_PER_SECOND};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Standard 24 fps.
pub const FPS_24: Rational32 = Rational32 { num: 24, den: 1 };
/// Standard 25 fps (PAL).
pub const FPS_25: Rational32 = Rational32 { num: 25, den: 1 };
/// Standard 30 fps.
pub const FPS_30: Rational32 = Rational32 { num: 30, den: 1 };
/// Standard 60 fps.
pub const FPS_60: Rational32 = Rational32 { num: 60, den: 1 };
/// NTSC 23.976 fps (`24000 / 1001`).
pub const FPS_23_976: Rational32 = Rational32 {
    num: 24_000,
    den: 1_001,
};
/// NTSC 29.97 fps (`30000 / 1001`).
pub const FPS_29_97: Rational32 = Rational32 {
    num: 30_000,
    den: 1_001,
};
/// NTSC 59.94 fps (`60000 / 1001`).
pub const FPS_59_94: Rational32 = Rational32 {
    num: 60_000,
    den: 1_001,
};

/// Standard 44.1 kHz audio sample rate.
pub const AUDIO_44_1_KHZ: u32 = 44_100;
/// Standard 48 kHz audio sample rate.
pub const AUDIO_48_KHZ: u32 = 48_000;
/// Standard 96 kHz audio sample rate.
pub const AUDIO_96_KHZ: u32 = 96_000;

/// Positive rational rate (`num / den` units per second), e.g. frames per second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rational32 {
    pub num: u32,
    pub den: u32,
}

impl Rational32 {
    /// Creates a validated non-zero `Rational32`.
    pub fn new(num: u32, den: u32) -> Result<Self, TimeError> {
        if num == 0 || den == 0 {
            return Err(TimeError::InvalidRational { num, den });
        }
        Ok(Self { num, den })
    }

    /// Returns a reduced copy with `gcd(num, den) == 1`.
    pub fn reduced(self) -> Result<Self, TimeError> {
        if self.num == 0 || self.den == 0 {
            return Err(TimeError::InvalidRational {
                num: self.num,
                den: self.den,
            });
        }
        let g = gcd_u32(self.num, self.den);
        Ok(Self {
            num: self.num / g,
            den: self.den / g,
        })
    }

    /// Converts the rational rate to `f64` for display purposes.
    #[inline]
    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    fn validate(self) -> Result<(), TimeError> {
        if self.num == 0 || self.den == 0 {
            return Err(TimeError::InvalidRational {
                num: self.num,
                den: self.den,
            });
        }
        Ok(())
    }

    /// Returns the exact duration of one frame in `Flicks` if `(FLICKS_PER_SECOND * den) % num == 0`.
    pub fn exact_frame_duration(self) -> Result<Option<Flicks>, TimeError> {
        self.validate()?;
        let numerator = (FLICKS_PER_SECOND as i128) * (self.den as i128);
        let denominator = self.num as i128;
        if numerator % denominator == 0 {
            let val = numerator / denominator;
            let ticks = i64::try_from(val).map_err(|_| TimeError::Overflow)?;
            Ok(Some(Flicks(ticks)))
        } else {
            Ok(None)
        }
    }

    /// Returns the duration of one frame in `Flicks` (exact for all standard rates,
    /// rounded to nearest tick otherwise) using `i128` intermediate math.
    pub fn frame_duration(self) -> Result<Flicks, TimeError> {
        self.frame_to_ticks_nearest(1)
    }

    /// Converts `frame_index` to `Flicks` using `i128` intermediate arithmetic (floor/Euclidean).
    ///
    /// For all standard integer and NTSC `1000/1001` rates (`24`, `25`, `30`, `60`,
    /// `23.976`, `29.97`, `59.94`), division is exact with zero remainder.
    pub fn frame_to_ticks(self, frame_index: i64) -> Result<Flicks, TimeError> {
        self.validate()?;
        let numerator = (frame_index as i128) * (FLICKS_PER_SECOND as i128) * (self.den as i128);
        let denominator = self.num as i128;
        let val = numerator.div_euclid(denominator);
        let ticks = i64::try_from(val).map_err(|_| TimeError::Overflow)?;
        Ok(Flicks(ticks))
    }

    /// Converts `frame_index` to `Flicks` rounded to nearest tick using exact `i128` arithmetic.
    pub fn frame_to_ticks_nearest(self, frame_index: i64) -> Result<Flicks, TimeError> {
        self.validate()?;
        let numerator = (frame_index as i128) * (FLICKS_PER_SECOND as i128) * (self.den as i128);
        let denominator = self.num as i128;
        let val = div_round_nearest_i128(numerator, denominator);
        let ticks = i64::try_from(val).map_err(|_| TimeError::Overflow)?;
        Ok(Flicks(ticks))
    }

    /// Converts `ticks` to the enclosing frame index (Euclidean floor) using `i128`.
    pub fn ticks_to_frame_floor(self, ticks: Flicks) -> Result<i64, TimeError> {
        self.validate()?;
        let numerator = (ticks.0 as i128) * (self.num as i128);
        let denominator = (FLICKS_PER_SECOND as i128) * (self.den as i128);
        let frame = numerator.div_euclid(denominator);
        i64::try_from(frame).map_err(|_| TimeError::Overflow)
    }

    /// Converts `ticks` to the nearest frame index using exact `i128` arithmetic.
    pub fn ticks_to_frame_nearest(self, ticks: Flicks) -> Result<i64, TimeError> {
        self.validate()?;
        let numerator = (ticks.0 as i128) * (self.num as i128);
        let denominator = (FLICKS_PER_SECOND as i128) * (self.den as i128);
        let frame = div_round_nearest_i128(numerator, denominator);
        i64::try_from(frame).map_err(|_| TimeError::Overflow)
    }

    /// Converts `ticks` to a frame index only if `ticks` falls on an exact frame boundary.
    pub fn ticks_to_frame_exact(self, ticks: Flicks) -> Result<Option<i64>, TimeError> {
        self.validate()?;
        let numerator = (ticks.0 as i128) * (self.num as i128);
        let denominator = (FLICKS_PER_SECOND as i128) * (self.den as i128);
        if numerator.rem_euclid(denominator) == 0 {
            let frame = numerator.div_euclid(denominator);
            Ok(Some(i64::try_from(frame).map_err(|_| TimeError::Overflow)?))
        } else {
            Ok(None)
        }
    }
}

impl fmt::Display for Rational32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            write!(f, "{} fps", self.num)
        } else {
            write!(f, "{}/{} fps ({:.3})", self.num, self.den, self.as_f64())
        }
    }
}

/// Converts an audio sample index at `sample_rate_hz` to `Flicks` using `i128` arithmetic.
///
/// Exact for `44_100 Hz` (`16_000` flicks/sample), `48_000 Hz` (`14_700` flicks/sample),
/// and `96_000 Hz` (`7_350` flicks/sample).
pub fn audio_samples_to_ticks(samples: i64, sample_rate_hz: u32) -> Result<Flicks, TimeError> {
    if sample_rate_hz == 0 {
        return Err(TimeError::InvalidSampleRate(sample_rate_hz));
    }
    let numerator = (samples as i128) * (FLICKS_PER_SECOND as i128);
    let denominator = sample_rate_hz as i128;
    let val = numerator.div_euclid(denominator);
    let ticks = i64::try_from(val).map_err(|_| TimeError::Overflow)?;
    Ok(Flicks(ticks))
}

/// Converts `Flicks` to the enclosing audio sample index (Euclidean floor) using `i128`.
pub fn ticks_to_audio_samples_floor(ticks: Flicks, sample_rate_hz: u32) -> Result<i64, TimeError> {
    if sample_rate_hz == 0 {
        return Err(TimeError::InvalidSampleRate(sample_rate_hz));
    }
    let numerator = (ticks.0 as i128) * (sample_rate_hz as i128);
    let denominator = FLICKS_PER_SECOND as i128;
    let samples = numerator.div_euclid(denominator);
    i64::try_from(samples).map_err(|_| TimeError::Overflow)
}

/// Converts `Flicks` to the nearest audio sample index using exact `i128` arithmetic.
pub fn ticks_to_audio_samples_nearest(
    ticks: Flicks,
    sample_rate_hz: u32,
) -> Result<i64, TimeError> {
    if sample_rate_hz == 0 {
        return Err(TimeError::InvalidSampleRate(sample_rate_hz));
    }
    let numerator = (ticks.0 as i128) * (sample_rate_hz as i128);
    let denominator = FLICKS_PER_SECOND as i128;
    let samples = div_round_nearest_i128(numerator, denominator);
    i64::try_from(samples).map_err(|_| TimeError::Overflow)
}

/// Converts `Flicks` to an exact audio sample index if it falls on an exact sample boundary.
pub fn ticks_to_audio_samples_exact(
    ticks: Flicks,
    sample_rate_hz: u32,
) -> Result<Option<i64>, TimeError> {
    if sample_rate_hz == 0 {
        return Err(TimeError::InvalidSampleRate(sample_rate_hz));
    }
    let numerator = (ticks.0 as i128) * (sample_rate_hz as i128);
    let denominator = FLICKS_PER_SECOND as i128;
    if numerator.rem_euclid(denominator) == 0 {
        let samples = numerator.div_euclid(denominator);
        Ok(Some(
            i64::try_from(samples).map_err(|_| TimeError::Overflow)?,
        ))
    } else {
        Ok(None)
    }
}

#[inline]
fn div_round_nearest_i128(num: i128, den: i128) -> i128 {
    debug_assert!(den > 0);
    let q = num.div_euclid(den);
    let r = num.rem_euclid(den);
    if r.saturating_mul(2) >= den {
        q + 1
    } else {
        q
    }
}

#[inline]
const fn gcd_u32(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}
