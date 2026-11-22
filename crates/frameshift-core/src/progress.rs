/// Render pipeline stage reported through
/// [`Engine::render_video_mp4_with_progress`](crate::Engine::render_video_mp4_with_progress).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RenderStage {
    /// Per-frame decode + GPU composite + encoder pipe loop.
    Frames,
    /// Project audio mix and optional range slice.
    AudioMix,
    /// MP4 mux/encode tail (the blocking `ffmpeg` flush).
    Encode,
}

/// One progress event emitted by the engine.
///
/// The library never formats or writes progress; callers own presentation. `completed`
/// counts finished work units inside `stage` and never exceeds `total`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderProgress {
    pub stage: RenderStage,
    pub completed: usize,
    pub total: usize,
}

impl RenderProgress {
    /// Creates a progress event for `stage`.
    pub const fn new(stage: RenderStage, completed: usize, total: usize) -> Self {
        Self {
            stage,
            completed,
            total,
        }
    }

    /// Creates a `Frames` stage event.
    pub const fn frames(completed: usize, total: usize) -> Self {
        Self::new(RenderStage::Frames, completed, total)
    }

    /// Creates an `AudioMix` stage event.
    pub const fn audio_mix(completed: usize, total: usize) -> Self {
        Self::new(RenderStage::AudioMix, completed, total)
    }

    /// Creates an `Encode` stage event.
    pub const fn encode(completed: usize, total: usize) -> Self {
        Self::new(RenderStage::Encode, completed, total)
    }
}
