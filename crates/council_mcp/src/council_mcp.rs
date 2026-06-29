/// council_mcp — stdio MCP server that bridges external agents to Zed Council.
///
/// Usage: COUNCIL_URL=ws://localhost:8080/rpc COUNCIL_TOKEN=<token> council_mcp
use anyhow::{Context as _, Result, bail};
use async_tungstenite::{
    tokio::connect_async,
    tungstenite::{Message as WsMessage, client::IntoClientRequest as _},
};
use futures::{SinkExt as _, StreamExt as _};
use proto::Envelope;
use prost::Message as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::{mpsc, oneshot};

// ---------------------------------------------------------------------------
// WebSocket actor
// ---------------------------------------------------------------------------

struct WsRequest {
    id: u32,
    payload: proto::envelope::Payload,
    reply: oneshot::Sender<Result<proto::envelope::Payload>>,
}

/// Background task owning the WebSocket connection.
/// Serializes send+recv to avoid locking issues.
async fn ws_actor<S>(mut ws: S, mut rx: mpsc::Receiver<WsRequest>)
where
    S: futures::Sink<WsMessage, Error = async_tungstenite::tungstenite::Error>
        + futures::Stream<Item = Result<WsMessage, async_tungstenite::tungstenite::Error>>
        + Unpin,
{
    let mut pending: HashMap<u32, oneshot::Sender<Result<proto::envelope::Payload>>> =
        HashMap::new();

    loop {
        tokio::select! {
            request = rx.recv() => {
                let Some(req) = request else { break };
                let envelope = Envelope {
                    id: req.id,
                    payload: Some(req.payload),
                    ..Default::default()
                };
                // Encode and send.
                let mut buf = Vec::new();
                if let Err(e) = envelope.encode(&mut buf) {
                    let _ = req.reply.send(Err(e.into()));
                    continue;
                }
                let compressed = match zstd::stream::encode_all(buf.as_slice(), -7) {
                    Ok(c) => c,
                    Err(e) => { let _ = req.reply.send(Err(e.into())); continue; }
                };
                if let Err(e) = ws.send(WsMessage::Binary(compressed.into())).await {
                    let _ = req.reply.send(Err(anyhow::anyhow!("{e}")));
                    continue;
                }
                pending.insert(req.id, req.reply);
            }
            msg = ws.next() => {
                let Some(msg) = msg else { break };
                let bytes = match msg {
                    Ok(WsMessage::Binary(b)) => b,
                    Ok(WsMessage::Ping(_)) | Ok(WsMessage::Pong(_)) => continue,
                    Ok(WsMessage::Close(_)) => break,
                    Ok(_) => continue,
                    Err(e) => {
                        for (_, tx) in pending.drain() {
                            let _ = tx.send(Err(anyhow::anyhow!("WebSocket error: {e}")));
                        }
                        break;
                    }
                };
                let mut decompressed = Vec::new();
                if let Err(e) = zstd::stream::copy_decode(bytes.as_ref(), &mut decompressed) {
                    eprintln!("zstd decode error: {e}");
                    continue;
                }
                let envelope = match Envelope::decode(decompressed.as_slice()) {
                    Ok(e) => e,
                    Err(e) => { eprintln!("proto decode error: {e}"); continue; }
                };
                if let Some(responding_to) = envelope.responding_to {
                    if let Some(tx) = pending.remove(&responding_to) {
                        let result = match envelope.payload {
                            Some(proto::envelope::Payload::Error(e)) => {
                                Err(anyhow::anyhow!("{}", e.message))
                            }
                            Some(p) => Ok(p),
                            None => Err(anyhow::anyhow!("empty response")),
                        };
                        let _ = tx.send(result);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Client stub
// ---------------------------------------------------------------------------

static MSG_ID: AtomicU32 = AtomicU32::new(1);

#[derive(Clone)]
struct CouncilClient {
    tx: mpsc::Sender<WsRequest>,
}

impl CouncilClient {
    async fn call(&self, payload: proto::envelope::Payload) -> Result<proto::envelope::Payload> {
        let id = MSG_ID.fetch_add(1, Ordering::Relaxed);
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(WsRequest { id, payload, reply: reply_tx })
            .await
            .map_err(|_| anyhow::anyhow!("actor stopped"))?;
        reply_rx.await.map_err(|_| anyhow::anyhow!("actor stopped"))?
    }
}

// ---------------------------------------------------------------------------
// MCP JSON-RPC types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct JsonRpcRequest {
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

impl JsonRpcResponse {
    fn ok(id: Option<Value>, result: Value) -> Self {
        Self { jsonrpc: "2.0", id, result: Some(result), error: None }
    }
    fn err(id: Option<Value>, code: i32, msg: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError { code, message: msg.into() }),
        }
    }
}

// ---------------------------------------------------------------------------
// Tool schema
// ---------------------------------------------------------------------------

fn tools_list() -> Value {
    json!({ "tools": [
        {
            "name": "council_join",
            "description": "Join or create a Council session for a project.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "integer" },
                    "role": { "type": "string", "enum": ["super", "supervisor", "peer"], "default": "peer" },
                    "agent_label": { "type": "string", "description": "Display name for this agent" },
                    "model": { "type": "string", "description": "LLM model identifier" },
                    "tool": { "type": "string", "description": "Tool name (e.g. claude_cli)" }
                },
                "required": ["project_id"]
            }
        },
        {
            "name": "council_read",
            "description": "Read session state, entries, and work items.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "integer" }
                },
                "required": ["session_id"]
            }
        },
        {
            "name": "council_post",
            "description": "Post an entry (Analysis / Critique / Proposal / Approval) to the council.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "integer" },
                    "kind": { "type": "string", "enum": ["analysis", "critique", "proposal", "approval"] },
                    "body": { "type": "string" },
                    "refs": { "type": "array", "items": { "type": "integer" }, "description": "Parent entry IDs" }
                },
                "required": ["session_id", "kind", "body"]
            }
        },
        {
            "name": "council_propose_tasks",
            "description": "Submit a task draft for approval (Supervisor only; Synthesize phase required).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "integer" },
                    "items": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "title": { "type": "string" },
                                "description": { "type": "string" },
                                "sort_order": { "type": "integer" }
                            },
                            "required": ["title", "sort_order"]
                        }
                    }
                },
                "required": ["session_id", "items"]
            }
        }
    ]})
}

// ---------------------------------------------------------------------------
// Tool handlers
// ---------------------------------------------------------------------------

async fn tool_council_join(args: &Value, client: &CouncilClient) -> Result<Value> {
    let project_id = args["project_id"].as_u64().context("project_id required")?;
    let role = match args["role"].as_str().unwrap_or("peer") {
        "super" => proto::CouncilParticipantKind::Super,
        "supervisor" => proto::CouncilParticipantKind::Supervisor,
        _ => proto::CouncilParticipantKind::Peer,
    };
    let resp = client
        .call(proto::envelope::Payload::JoinCouncil(proto::JoinCouncil {
            project_id,
            kind: role as i32,
            agent_label: args["agent_label"].as_str().unwrap_or("").to_string(),
            model: args["model"].as_str().unwrap_or("").to_string(),
            tool: args["tool"].as_str().unwrap_or("council_mcp").to_string(),
        }))
        .await?;
    match resp {
        proto::envelope::Payload::JoinCouncilResponse(r) => Ok(json!({
            "participant_id": r.participant_id,
            "replica_id": r.replica_id,
            "session": r.state.as_ref().and_then(|s| s.session.as_ref()).map(|s| json!({
                "id": s.id,
                "phase": format!("{:?}", s.phase()),
                "authority": format!("{:?}", s.authority()),
            })),
            "participant_count": r.state.as_ref().map(|s| s.participants.len()),
        })),
        _ => bail!("unexpected response"),
    }
}

async fn tool_council_read(args: &Value, client: &CouncilClient) -> Result<Value> {
    let session_id = args["session_id"].as_u64().context("session_id required")?;
    let resp = client
        .call(proto::envelope::Payload::GetCouncilState(
            proto::GetCouncilState { session_id },
        ))
        .await?;
    match resp {
        proto::envelope::Payload::GetCouncilStateResponse(r) => {
            let state = r.state.as_ref();
            Ok(json!({
                "session": state.and_then(|s| s.session.as_ref()).map(|s| json!({
                    "id": s.id,
                    "phase": format!("{:?}", s.phase()),
                    "authority": format!("{:?}", s.authority()),
                    "round": s.round,
                })),
                "participants": state.map(|s| s.participants.iter().map(|p| json!({
                    "id": p.id,
                    "kind": format!("{:?}", p.kind()),
                    "label": &p.agent_label,
                })).collect::<Vec<_>>()),
                "entries": state.map(|s| s.entries.iter().map(|e| json!({
                    "id": e.id,
                    "kind": format!("{:?}", e.kind()),
                    "body": &e.body,
                    "lamport": e.lamport_value,
                    "author_participant_id": e.author_participant_id,
                })).collect::<Vec<_>>()),
                "work_items": state.map(|s| s.work_items.iter().map(|w| json!({
                    "id": w.id,
                    "title": &w.title,
                    "description": &w.description,
                    "status": format!("{:?}", w.status()),
                })).collect::<Vec<_>>()),
            }))
        }
        _ => bail!("unexpected response"),
    }
}

async fn tool_council_post(args: &Value, client: &CouncilClient) -> Result<Value> {
    let session_id = args["session_id"].as_u64().context("session_id required")?;
    let body = args["body"].as_str().context("body required")?.to_string();
    let kind = match args["kind"].as_str().unwrap_or("analysis") {
        "critique" => proto::CouncilEntryKind::Critique,
        "proposal" => proto::CouncilEntryKind::Proposal,
        "approval" => proto::CouncilEntryKind::Approval,
        _ => proto::CouncilEntryKind::Analysis,
    };
    let refs: Vec<u64> = args["refs"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
        .unwrap_or_default();
    let resp = client
        .call(proto::envelope::Payload::PostCouncilEntry(
            proto::PostCouncilEntry { session_id, kind: kind as i32, body, refs },
        ))
        .await?;
    match resp {
        proto::envelope::Payload::PostCouncilEntryResponse(r) => Ok(json!({
            "entry_id": r.entry.as_ref().map(|e| e.id),
            "lamport": r.entry.as_ref().map(|e| e.lamport_value),
        })),
        _ => bail!("unexpected response"),
    }
}

async fn tool_council_propose_tasks(args: &Value, client: &CouncilClient) -> Result<Value> {
    let session_id = args["session_id"].as_u64().context("session_id required")?;
    let items = args["items"]
        .as_array()
        .context("items required")?
        .iter()
        .map(|v| proto::WorkItem {
            title: v["title"].as_str().unwrap_or("").to_string(),
            description: v["description"].as_str().unwrap_or("").to_string(),
            sort_order: v["sort_order"].as_i64().unwrap_or(0) as i32,
            ..Default::default()
        })
        .collect();
    let resp = client
        .call(proto::envelope::Payload::SubmitTaskDraft(
            proto::SubmitTaskDraft { session_id, items },
        ))
        .await?;
    match resp {
        proto::envelope::Payload::SubmitTaskDraftResponse(r) => Ok(json!({
            "draft_entry_id": r.draft_entry_id,
            "item_count": r.items.len(),
            "note": "Draft submitted. Session is now in Gate phase awaiting Super approval."
        })),
        _ => bail!("unexpected response"),
    }
}

async fn dispatch_tool(name: &str, args: &Value, client: &CouncilClient) -> Result<Value> {
    match name {
        "council_join" => tool_council_join(args, client).await,
        "council_read" => tool_council_read(args, client).await,
        "council_post" => tool_council_post(args, client).await,
        "council_propose_tasks" => tool_council_propose_tasks(args, client).await,
        other => bail!("unknown tool: {other}"),
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::var("COUNCIL_URL")
        .unwrap_or_else(|_| "ws://localhost:8080/rpc".to_string());
    let token = std::env::var("COUNCIL_TOKEN").unwrap_or_default();

    let mut request = url.as_str().into_client_request()?;
    if !token.is_empty() {
        request
            .headers_mut()
            .insert("x-zed-access-token", token.parse()?);
    }

    let (ws, _) = connect_async(request)
        .await
        .with_context(|| format!("failed to connect to {url}"))?;

    let (tx, rx) = mpsc::channel::<WsRequest>(32);
    tokio::spawn(ws_actor(ws, rx));

    let client = CouncilClient { tx };

    use tokio::io::{AsyncBufReadExt as _, BufReader};
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                emit(&mut stdout, JsonRpcResponse::err(None, -32700, format!("parse error: {e}")))
                    .await?;
                continue;
            }
        };

        let id = req.id.clone();
        let resp = match req.method.as_str() {
            "initialize" => JsonRpcResponse::ok(
                id,
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "council_mcp", "version": "0.1.0" }
                }),
            ),
            "notifications/initialized" => continue,
            "tools/list" => JsonRpcResponse::ok(id, tools_list()),
            "tools/call" => {
                let tool = req.params.as_ref()
                    .and_then(|p| p["name"].as_str())
                    .unwrap_or("")
                    .to_string();
                let args = req.params.as_ref()
                    .and_then(|p| p.get("arguments"))
                    .cloned()
                    .unwrap_or(Value::Null);
                match dispatch_tool(&tool, &args, &client).await {
                    Ok(v) => JsonRpcResponse::ok(
                        id,
                        json!({"content":[{"type":"text","text":v.to_string()}]}),
                    ),
                    Err(e) => JsonRpcResponse::err(id, -32000, e.to_string()),
                }
            }
            other => JsonRpcResponse::err(id, -32601, format!("method not found: {other}")),
        };

        emit(&mut stdout, resp).await?;
    }

    Ok(())
}

async fn emit(stdout: &mut tokio::io::Stdout, resp: JsonRpcResponse) -> Result<()> {
    use tokio::io::AsyncWriteExt as _;
    let mut line = serde_json::to_string(&resp)?;
    line.push('\n');
    stdout.write_all(line.as_bytes()).await?;
    stdout.flush().await?;
    Ok(())
}
