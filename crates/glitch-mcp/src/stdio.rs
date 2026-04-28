//! Subprocess entry point. Run when glitch.exe is invoked with
//! `--mcp-permission-server <pipe-name>`.
//!
//! Speaks MCP/JSON-RPC over stdio to Claude Code. For each `tools/call approve`
//! request, forwards a [`PipeMessage::Request`] over the named pipe to the
//! main Glitch UI and writes the resulting [`ApprovalDecision`] back to Claude
//! as a JSON string in a text content block.

use crate::pipe::{ApprovalDecision, ApprovalRequest, PipeMessage};
use crate::proto::{
    error_response, ok_response, ContentBlock, InitializeResult, JsonRpcRequest, ServerInfo,
    ToolCallParams, ToolCallResult, ToolDef, ToolsListResult,
};
use serde_json::json;
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader as TokioBufReader, BufWriter};
use tokio::net::windows::named_pipe::ClientOptions;
use tokio::sync::Mutex;

const PROTOCOL_VERSION: &str = "2024-11-05";
const TOOL_NAME: &str = "approve";

#[derive(Debug, Error)]
pub enum StdioError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub async fn run_permission_stdio(pipe_name: &str) -> Result<(), StdioError> {
    let pipe = ClientOptions::new().open(pipe_name)?;
    let (pipe_read, pipe_write) = tokio::io::split(pipe);
    let pipe_reader = Arc::new(Mutex::new(TokioBufReader::new(pipe_read)));
    let pipe_writer = Arc::new(Mutex::new(BufWriter::new(pipe_write)));

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = TokioBufReader::new(stdin);
    let mut writer = BufWriter::new(stdout);

    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let request: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!("malformed JSON-RPC line: {err}");
                continue;
            }
        };

        match request.method.as_str() {
            "initialize" => {
                let result = InitializeResult {
                    protocol_version: PROTOCOL_VERSION.into(),
                    capabilities: json!({"tools": {}}),
                    server_info: ServerInfo {
                        name: crate::SERVER_NAME.into(),
                        version: env!("CARGO_PKG_VERSION").into(),
                    },
                };
                if let Some(id) = request.id {
                    let response = ok_response(id, serde_json::to_value(result)?);
                    write_response(&mut writer, &response).await?;
                }
            }
            "notifications/initialized" | "initialized" => {
                // Notification — no response.
            }
            "tools/list" => {
                let result = ToolsListResult {
                    tools: vec![ToolDef {
                        name: TOOL_NAME.into(),
                        description:
                            "Prompt the user in Glitch to approve or deny a Claude tool call."
                                .into(),
                        input_schema: json!({
                            "type": "object",
                            "properties": {
                                "tool_name": {"type": "string"},
                                "input": {"type": "object"}
                            },
                            "required": ["tool_name"]
                        }),
                    }],
                };
                if let Some(id) = request.id {
                    let response = ok_response(id, serde_json::to_value(result)?);
                    write_response(&mut writer, &response).await?;
                }
            }
            "tools/call" => {
                let id = match request.id {
                    Some(v) => v,
                    None => continue,
                };
                let params: ToolCallParams = match request
                    .params
                    .ok_or_else(|| StdioError::Json(serde::de::Error::custom("missing params")))
                {
                    Ok(v) => match serde_json::from_value(v) {
                        Ok(p) => p,
                        Err(err) => {
                            let response =
                                error_response(id, -32602, format!("invalid params: {err}"));
                            write_response(&mut writer, &response).await?;
                            continue;
                        }
                    },
                    Err(err) => {
                        let response = error_response(id, -32602, err.to_string());
                        write_response(&mut writer, &response).await?;
                        continue;
                    }
                };

                if params.name != TOOL_NAME {
                    let response = error_response(id, -32601, format!("unknown tool: {}", params.name));
                    write_response(&mut writer, &response).await?;
                    continue;
                }

                let approval_req: ApprovalRequest = match serde_json::from_value(params.arguments) {
                    Ok(v) => v,
                    Err(err) => {
                        let response =
                            error_response(id, -32602, format!("invalid arguments: {err}"));
                        write_response(&mut writer, &response).await?;
                        continue;
                    }
                };

                let decision = match forward_to_pipe(
                    pipe_reader.clone(),
                    pipe_writer.clone(),
                    approval_req,
                )
                .await
                {
                    Ok(d) => d,
                    Err(err) => {
                        tracing::warn!("pipe error: {err}");
                        ApprovalDecision::deny(format!("Glitch UI not reachable: {err}"))
                    }
                };

                let payload = serde_json::to_string(&decision)?;
                let result = ToolCallResult {
                    content: vec![ContentBlock::Text { text: payload }],
                    is_error: None,
                };
                let response = ok_response(id, serde_json::to_value(result)?);
                write_response(&mut writer, &response).await?;
            }
            other => {
                tracing::debug!("unhandled MCP method: {other}");
                if let Some(id) = request.id {
                    let response = error_response(id, -32601, format!("method not found: {other}"));
                    write_response(&mut writer, &response).await?;
                }
            }
        }
    }

    Ok(())
}

async fn forward_to_pipe(
    reader: Arc<Mutex<TokioBufReader<tokio::io::ReadHalf<tokio::net::windows::named_pipe::NamedPipeClient>>>>,
    writer: Arc<Mutex<BufWriter<tokio::io::WriteHalf<tokio::net::windows::named_pipe::NamedPipeClient>>>>,
    req: ApprovalRequest,
) -> Result<ApprovalDecision, StdioError> {
    // Hold on to the original input so we can echo it as `updatedInput` if
    // the UI accepts the call unchanged. Claude Code's Zod schema requires
    // `updatedInput` to be present (as a record) on the Allow shape.
    let original_input = req.input.clone();

    let id = ulid::Ulid::new().to_string();
    let request_msg = PipeMessage::Request {
        id: id.clone(),
        tool_name: req.tool_name,
        input: req.input,
    };
    let serialized = serde_json::to_string(&request_msg)?;

    {
        let mut w = writer.lock().await;
        w.write_all(serialized.as_bytes()).await?;
        w.write_all(b"\n").await?;
        w.flush().await?;
    }

    let mut buf = String::new();
    loop {
        buf.clear();
        let n = {
            let mut r = reader.lock().await;
            r.read_line(&mut buf).await?
        };
        if n == 0 {
            return Err(StdioError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "pipe closed before response",
            )));
        }
        let msg: PipeMessage = serde_json::from_str(buf.trim())?;
        if let PipeMessage::Response {
            id: resp_id,
            decision,
        } = msg
        {
            if resp_id == id {
                let mut decision = decision;
                if let ApprovalDecision::Allow { updated_input } = &mut decision {
                    if updated_input.is_none() {
                        // Echo the original tool input so Claude's schema is satisfied.
                        // If for some reason the original wasn't an object, fall back
                        // to an empty object so the field is still a "record".
                        let echoed = match &original_input {
                            serde_json::Value::Object(_) => original_input.clone(),
                            _ => serde_json::Value::Object(serde_json::Map::new()),
                        };
                        *updated_input = Some(echoed);
                    }
                }
                return Ok(decision);
            }
        }
    }
}

async fn write_response<W: AsyncWriteExt + Unpin>(
    writer: &mut BufWriter<W>,
    response: &crate::proto::JsonRpcResponse,
) -> Result<(), StdioError> {
    let line = serde_json::to_string(response)?;
    writer.write_all(line.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}
