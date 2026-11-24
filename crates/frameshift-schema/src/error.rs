use crate::graph::NodeId;
use crate::version::SchemaVersion;
use frameshift_time::Flicks;
use thiserror::Error;

/// Errors produced by `frameshift-schema` validation and session mutations.
#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("unsupported schema version {0}; expected {expected}", expected = crate::version::CURRENT_SCHEMA_VERSION)]
    UnsupportedSchemaVersion(SchemaVersion),

    #[error("revision conflict: expected revision {expected}, actual revision {actual}")]
    RevisionConflict { expected: u64, actual: u64 },

    #[error("duplicate {entity} id '{id}'")]
    DuplicateId { entity: &'static str, id: String },

    #[error("missing asset '{0}'")]
    MissingAsset(String),

    #[error("missing clip '{0}'")]
    MissingClip(String),

    #[error("missing event '{0}'")]
    MissingEvent(String),

    #[error("missing graph node '{0}'")]
    MissingNode(NodeId),

    #[error("invalid clip bounds for '{clip_id}': start ({start}) must be < end ({end})")]
    InvalidClipBounds {
        clip_id: String,
        start: Flicks,
        end: Flicks,
    },

    #[error("invalid timeline: {0}")]
    InvalidTimeline(String),

    #[error("node '{node_id}' expects {expected} input(s), found {actual}")]
    InvalidNodeArity {
        node_id: NodeId,
        expected: usize,
        actual: usize,
    },

    #[error("invalid custom_wgsl node '{node_id}': {message}")]
    InvalidCustomWgsl { node_id: NodeId, message: String },

    #[error("invalid text3_d node '{node_id}': {message}")]
    InvalidText3D { node_id: NodeId, message: String },

    #[error("invalid scene3_d node '{node_id}': {message}")]
    InvalidScene3D { node_id: NodeId, message: String },

    #[error("invalid shape3_d node '{node_id}': {message}")]
    InvalidShape3D { node_id: NodeId, message: String },

    #[error("invalid lut3_d node '{node_id}': {message}")]
    InvalidLut3D { node_id: NodeId, message: String },

    #[error("invalid precomp node '{node_id}': {message}")]
    InvalidPrecomp { node_id: NodeId, message: String },

    #[error("invalid precomp authoring operation: {message}")]
    InvalidPrecompOperation { message: String },

    #[error("invalid template '{template_id}': {message}")]
    InvalidTemplate {
        template_id: String,
        message: String,
    },

    #[error("template slot '{0}' is required but has no value")]
    MissingTemplateSlot(String),

    #[error("template values contain unknown slot '{0}'")]
    UnknownTemplateSlot(String),

    #[error("template slot '{slot_id}' has an invalid value: {message}")]
    InvalidTemplateSlot { slot_id: String, message: String },

    #[error("template '{0}' has not been reviewed")]
    UnreviewedTemplate(String),

    #[error("precomp node '{node_id}' depth ({depth}) exceeds maximum ({max_depth})")]
    PrecompDepthExceeded {
        node_id: NodeId,
        depth: usize,
        max_depth: usize,
    },

    #[error("invalid vector_text node '{node_id}': {message}")]
    InvalidVectorText { node_id: NodeId, message: String },

    #[error("invalid grid_warp node '{node_id}': {message}")]
    InvalidGridWarp { node_id: NodeId, message: String },

    #[error("invalid corner_pin node '{node_id}': {message}")]
    InvalidCornerPin { node_id: NodeId, message: String },

    #[error("invalid vector_path_hud node '{node_id}': {message}")]
    InvalidVectorPathHud { node_id: NodeId, message: String },

    #[error("invalid mask_matte node '{node_id}': {message}")]
    InvalidMaskMatte { node_id: NodeId, message: String },

    #[error("invalid transform3_d node '{node_id}': {message}")]
    InvalidTransform3D { node_id: NodeId, message: String },

    #[error("invalid adjustment_layer node '{node_id}': {message}")]
    InvalidAdjustmentLayer { node_id: NodeId, message: String },

    #[error("invalid blur_mo_curves node '{node_id}': {message}")]
    InvalidBlurMoCurves { node_id: NodeId, message: String },

    #[error("invalid null3_d node '{node_id}': {message}")]
    InvalidNull3D { node_id: NodeId, message: String },

    #[error("invalid depth_matte node '{node_id}': {message}")]
    InvalidDepthMatte { node_id: NodeId, message: String },

    #[error("render graph contains a cycle involving nodes: {cycle:?}")]
    CycleDetected { cycle: Vec<NodeId> },

    #[error("invalid parameter binding on node '{node_id}' parameter '{param_name}': {message}")]
    InvalidParamBinding {
        node_id: NodeId,
        param_name: String,
        message: String,
    },

    #[error("invalid proxy config: {0}")]
    InvalidProxyConfig(String),

    #[error("cannot remove asset '{0}' while referenced by timeline clips")]
    AssetInUse(String),

    #[error("cannot remove node '{0}' while referenced as output or input")]
    NodeInUse(NodeId),

    #[error("nothing to undo")]
    EmptyUndoStack,

    #[error("nothing to redo")]
    EmptyRedoStack,

    #[error("JSON serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
