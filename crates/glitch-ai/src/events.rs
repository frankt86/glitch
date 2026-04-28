use serde::{Deserialize, Serialize};

/// A single line emitted by `claude -p --output-format stream-json`.
///
/// Claude Code's stream-json schema is not formally stable; we model the
/// fields we care about and capture the rest in a passthrough JSON Value so
/// unknown variants don't crash the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// Initial handshake — session id, model, available tools.
    System {
        #[serde(default)]
        subtype: Option<String>,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        tools: Vec<serde_json::Value>,
        #[serde(flatten)]
        extra: serde_json::Map<String, serde_json::Value>,
    },
    /// Assistant turn — text deltas, thinking blocks, or tool_use blocks.
    Assistant {
        message: AssistantMessage,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(flatten)]
        extra: serde_json::Map<String, serde_json::Value>,
    },
    /// User-side echo, including tool_result blocks once the harness has run a tool.
    User {
        message: UserMessage,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(flatten)]
        extra: serde_json::Map<String, serde_json::Value>,
    },
    /// Final summary at end of turn.
    Result {
        #[serde(default)]
        subtype: Option<String>,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        is_error: bool,
        #[serde(default)]
        result: Option<String>,
        #[serde(default)]
        total_cost_usd: Option<f64>,
        #[serde(default)]
        duration_ms: Option<u64>,
        #[serde(flatten)]
        extra: serde_json::Map<String, serde_json::Value>,
    },
    /// Catch-all for unknown event types; kept so the UI can ignore gracefully.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssistantMessage {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub content: Vec<ContentBlock>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserMessage {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: ContentField,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// User messages can carry either a plain string or a list of content blocks
/// (e.g. tool_result). We accept both shapes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentField {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

impl Default for ContentField {
    fn default() -> Self {
        Self::Blocks(Vec::new())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Thinking {
        #[serde(default)]
        thinking: String,
    },
    ToolUse {
        id: String,
        name: String,
        #[serde(default)]
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        #[serde(default)]
        content: serde_json::Value,
        #[serde(default)]
        is_error: bool,
    },
    #[serde(other)]
    Unknown,
}

/// What the UI sends *into* claude's stdin. Stream-json input wraps every
/// turn in `{"type":"user","message":{"role":"user","content":"..."}}`.
#[derive(Debug, Clone, Serialize)]
pub struct UserInput<'a> {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub message: UserInputMessage<'a>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserInputMessage<'a> {
    pub role: &'static str,
    pub content: &'a str,
}

impl<'a> UserInput<'a> {
    pub fn user(content: &'a str) -> Self {
        Self {
            kind: "user",
            message: UserInputMessage {
                role: "user",
                content,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_system_init() {
        let line = r#"{"type":"system","subtype":"init","session_id":"abc","model":"claude-opus-4-7","tools":[]}"#;
        let event: StreamEvent = serde_json::from_str(line).unwrap();
        assert!(matches!(event, StreamEvent::System { .. }));
    }

    #[test]
    fn parses_assistant_with_tool_use() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"reading"},{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/tmp/x"}}]},"session_id":"abc"}"#;
        let event: StreamEvent = serde_json::from_str(line).unwrap();
        match event {
            StreamEvent::Assistant { message, .. } => {
                assert_eq!(message.content.len(), 2);
                assert!(matches!(message.content[0], ContentBlock::Text { .. }));
                assert!(matches!(message.content[1], ContentBlock::ToolUse { .. }));
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn parses_user_tool_result() {
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"file contents","is_error":false}]}}"#;
        let event: StreamEvent = serde_json::from_str(line).unwrap();
        match event {
            StreamEvent::User { message, .. } => match message.content {
                ContentField::Blocks(blocks) => {
                    assert_eq!(blocks.len(), 1);
                    assert!(matches!(blocks[0], ContentBlock::ToolResult { .. }));
                }
                _ => panic!("expected blocks"),
            },
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn parses_result() {
        let line = r#"{"type":"result","subtype":"success","session_id":"abc","is_error":false,"result":"done","total_cost_usd":0.01,"duration_ms":1500}"#;
        let event: StreamEvent = serde_json::from_str(line).unwrap();
        assert!(matches!(event, StreamEvent::Result { .. }));
    }

    #[test]
    fn unknown_event_is_unknown() {
        let line = r#"{"type":"future_event_type","data":{}}"#;
        let event: StreamEvent = serde_json::from_str(line).unwrap();
        assert!(matches!(event, StreamEvent::Unknown));
    }
}
