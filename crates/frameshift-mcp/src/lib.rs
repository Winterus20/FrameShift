//! JSON-RPC 2.0 MCP bridge and autonomous Agent Director verification for FrameShift.

pub mod error;
pub mod server;
pub mod verification;

pub use error::McpError;
pub use server::{
    McpServer, McpToolDescriptor, MutateProjectResult, PreferenceQueryResult, RenderProjectResult,
    SchemaDescription,
};
pub use verification::{run_m3_agent_director_verification, M3VerificationSummary};

/// Returns the crate identifier string.
pub fn crate_name() -> &'static str {
    "frameshift-mcp"
}
