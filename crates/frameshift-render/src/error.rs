use frameshift_schema::{NodeId, SchemaError};
use frameshift_time::TimeError;
use thiserror::Error;

/// Errors raised by headless GPU initialization, shader compilation, or DAG rendering.
#[derive(Debug, Error)]
pub enum RenderError {
    #[error("no suitable wgpu adapter found: {0}")]
    NoGpuAdapter(String),

    #[error("failed to request wgpu device: {0}")]
    DeviceRequest(#[from] wgpu::RequestDeviceError),

    #[error("WGSL shader compilation failed for '{shader}': {message}")]
    ShaderCompilation { shader: String, message: String },

    #[error("invalid render dimensions {width}x{height} (must be non-zero)")]
    InvalidDimensions { width: u32, height: u32 },

    #[error("invalid motion blur parameters: {0}")]
    InvalidMotionBlurParams(String),

    #[error("missing graph input for node '{node_id}': expected {expected}, got {actual}")]
    MissingGraphInput {
        node_id: NodeId,
        expected: usize,
        actual: usize,
    },

    #[error("unsupported shader transition '{0}'")]
    UnsupportedShaderTransition(String),

    #[error("invalid 3D LUT: {0}")]
    InvalidLut3D(String),

    #[error("frame provider error for asset '{asset_id}' at tick {source_tick}: {message}")]
    FrameProvider {
        asset_id: String,
        source_tick: i64,
        message: String,
    },

    #[error(
        "source frame byte/pixel buffer size mismatch for '{asset_id}': expected {expected}, got {actual}"
    )]
    InvalidFrameBuffer {
        asset_id: String,
        expected: usize,
        actual: usize,
    },

    #[error("GPU buffer mapping failed: {0}")]
    BufferMap(String),

    #[error(
        "expression evaluation failed for node '{node_id}' parameter '{param_name}': {message}"
    )]
    Expression {
        node_id: NodeId,
        param_name: String,
        message: String,
    },

    #[error("schema validation error: {0}")]
    Schema(#[from] SchemaError),

    #[error("timebase error: {0}")]
    Time(#[from] TimeError),

    #[error("vector/font error: {0}")]
    Vector(#[from] frameshift_vector::VectorError),
}
