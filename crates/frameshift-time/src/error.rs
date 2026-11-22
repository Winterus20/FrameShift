use crate::expr::ExprError;
use crate::impact_solver::ConstraintViolation;
use crate::Flicks;
use thiserror::Error;

/// Errors produced by `frameshift-time` operations.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum TimeError {
    #[error("invalid rational rate {num}/{den}: numerator and denominator must be non-zero")]
    InvalidRational { num: u32, den: u32 },

    #[error("invalid audio sample rate {0} Hz: must be non-zero")]
    InvalidSampleRate(u32),

    #[error("integer overflow during tick conversion")]
    Overflow,

    #[error("PTS index is not monotonically non-decreasing at index {index}: {prev} > {current}")]
    UnsortedPts {
        index: usize,
        prev: Flicks,
        current: Flicks,
    },

    #[error("invalid speed curve: {0}")]
    InvalidSpeedCurve(String),

    #[error("impact timing constraint violation: {0}")]
    ConstraintViolation(#[from] ConstraintViolation),

    #[error("expression error: {0}")]
    Expr(#[from] ExprError),
}
