//! Minimal MCP (Model Context Protocol) implementation: just enough to act as
//! the `--permission-prompt-tool` for Claude Code. The same glitch.exe
//! re-invokes itself via `--mcp-permission-server <pipe>`; in that mode, this
//! crate's [`run_permission_stdio`] takes over: it speaks MCP on stdio to
//! Claude and forwards each `tools/call approve` request to the main Glitch
//! app over the given Windows named pipe.

pub mod pipe;
pub mod proto;

#[cfg(windows)]
pub mod stdio;

pub use pipe::{ApprovalDecision, ApprovalRequest, PipeMessage};
pub use proto::{ContentBlock, JsonRpcError, JsonRpcRequest, JsonRpcResponse};

#[cfg(windows)]
pub use stdio::run_permission_stdio;

/// Tool name as Claude Code expects it: `mcp__<server>__<tool>`.
pub const PERMISSION_TOOL_NAME: &str = "mcp__glitch_permissions__approve";

/// MCP server name used in the config + tool path.
pub const SERVER_NAME: &str = "glitch_permissions";
