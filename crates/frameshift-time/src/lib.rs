//! Deterministic Flicks timebase, rational FPS, VFR PTS indexing,
//! numerical speed-curve integration, and semantic impact constraint solving.

pub mod error;
pub mod expr;
pub mod flicks;
pub mod impact_solver;
pub mod pts_index;
pub mod rational;
pub mod speed_curve;

pub use error::TimeError;
pub use expr::{
    ExprContext, ExprError, SafeExpr, DEFAULT_MAX_FUEL, MAX_EXPR_DEPTH, MAX_EXPR_NODES,
};
pub use flicks::{Flicks, FLICKS_PER_MILLISECOND, FLICKS_PER_SECOND};
pub use impact_solver::{
    anime_snap_in, anime_snap_out, twixtor_ramp, ConstraintFailure, ConstraintViolation,
    ImpactCurveStyle, ImpactSolution, ImpactSolveRequest, ImpactSolver, SegmentSide,
    SourceFitMode, SuggestedAlternative,
};
pub use pts_index::{FrameMatch, PtsIndex};
pub use rational::{
    audio_samples_to_ticks, ticks_to_audio_samples_exact, ticks_to_audio_samples_floor,
    ticks_to_audio_samples_nearest, Rational32, AUDIO_44_1_KHZ, AUDIO_48_KHZ, AUDIO_96_KHZ,
    FPS_23_976, FPS_24, FPS_25, FPS_29_97, FPS_30, FPS_59_94, FPS_60,
};
pub use speed_curve::{
    apply_time_quantize, BezierSpeedKeyframe, KeyframeEase, SpeedCurve, SpeedCurveSpec,
    SpeedKeyframe, SpeedLutEntry, TimeQuantizeSpec,
};
