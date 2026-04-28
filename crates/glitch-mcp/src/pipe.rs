//! Wire format used between the MCP-permission subprocess (spawned by Claude)
//! and the main Glitch UI process. Newline-delimited JSON over a Windows named
//! pipe. Independent of the MCP/JSON-RPC framing on Claude's side.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What Claude actually puts in the `arguments` of the `approve` tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub tool_name: String,
    #[serde(default)]
    pub input: Value,
}

/// Format Claude expects to see returned (as a JSON string in a text block).
///
/// Note: `updatedInput` is required-on-the-wire for Allow per Claude Code's
/// Zod schema. We keep it `Option` here so callers (e.g. the UI handler) can
/// signal "no modification" with `None`; the MCP subprocess fills in the
/// original input before serializing to Claude.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "behavior", rename_all = "lowercase")]
pub enum ApprovalDecision {
    Allow {
        #[serde(rename = "updatedInput", default)]
        updated_input: Option<Value>,
    },
    Deny {
        #[serde(default)]
        message: String,
    },
}

impl ApprovalDecision {
    pub fn allow_unchanged() -> Self {
        Self::Allow {
            updated_input: None,
        }
    }
    pub fn deny(reason: impl Into<String>) -> Self {
        Self::Deny {
            message: reason.into(),
        }
    }
}

/// Pipe-side framing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PipeMessage {
    Request {
        id: String,
        tool_name: String,
        #[serde(default)]
        input: Value,
    },
    Response {
        id: String,
        decision: ApprovalDecision,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn allow_with_input_serializes_with_updated_input() {
        let d = ApprovalDecision::Allow {
            updated_input: Some(json!({"file_path": "/tmp/x"})),
        };
        let s = serde_json::to_string(&d).unwrap();
        assert!(s.contains("\"behavior\":\"allow\""), "{s}");
        assert!(s.contains("\"updatedInput\""), "{s}");
        assert!(s.contains("\"/tmp/x\""), "{s}");
    }

    #[test]
    fn deny_serializes_with_message() {
        let d = ApprovalDecision::deny("user denied");
        let s = serde_json::to_string(&d).unwrap();
        assert_eq!(s, r#"{"behavior":"deny","message":"user denied"}"#);
    }

    #[test]
    fn allow_with_none_input_omits_field() {
        // Sanity check: this is the unpatched shape that broke Claude. The
        // fix is in stdio.rs::forward_to_pipe — this test documents the
        // raw serializer behavior so anyone changing the type is forced to
        // remember the patch step.
        let d = ApprovalDecision::Allow { updated_input: None };
        let s = serde_json::to_string(&d).unwrap();
        assert_eq!(s, r#"{"behavior":"allow","updatedInput":null}"#);
    }
}
