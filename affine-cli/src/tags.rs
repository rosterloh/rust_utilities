// Tag management via the AFFiNE sync protocol.
//
// Tags are stored in the workspace root Yjs document's `meta` map:
//
//   meta.properties.tags.options = [
//     { id: string, value: string, color: string, createDate, updateDate }
//   ]
//   meta.pages[pageId].tags = [tagId1, tagId2, ...]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use rust_socketio::asynchronous::{Client, ClientBuilder};
use rust_socketio::{Payload, TransportType};
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio::time::timeout;
use url::Url;

use crate::client::AffineClient;
use crate::yjs_engine;

const LOAD_TIMEOUT: Duration = Duration::from_secs(30);
const JOIN_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TagOption {
    pub id: String,
    pub value: String,
    pub color: String,
    pub create_date: Option<f64>,
    pub update_date: Option<f64>,
}

fn scripts_dir() -> Result<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scripts = manifest.join("scripts");
    if scripts.exists() {
        Ok(scripts)
    } else {
        let exe = std::env::current_exe()?;
        let project_root = exe
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .map(|p| p.join("affine-cli").join("scripts"));
        if let Some(p) = project_root {
            if p.exists() {
                return Ok(p);
            }
        }
        bail!("scripts directory not found at {}", scripts.display())
    }
}

// ── Public API ─────────────────────────────────────────────────────────────

pub async fn list_tags(client: &AffineClient, workspace_id: &str) -> Result<Vec<TagOption>> {
    let b64 = load_root_doc(client, workspace_id).await?;
    let scripts = scripts_dir()?;
    let tags = yjs_engine::list_tags_op(&scripts, &b64)?;
    Ok(tags)
}

pub async fn create_tag(
    client: &AffineClient,
    workspace_id: &str,
    name: &str,
    color: &str,
) -> Result<TagOption> {
    let b64 = load_root_doc(client, workspace_id).await?;
    let scripts = scripts_dir()?;
    let (tag, new_state) = yjs_engine::create_tag_op(&scripts, &b64, name, color)?;
    push_root_doc_update(client, workspace_id, &new_state).await?;
    Ok(tag)
}

pub async fn assign_tag(
    client: &AffineClient,
    workspace_id: &str,
    doc_id: &str,
    tag_id: &str,
) -> Result<()> {
    let b64 = load_root_doc(client, workspace_id).await?;
    let scripts = scripts_dir()?;
    let new_state = yjs_engine::assign_tag_op(&scripts, &b64, doc_id, tag_id)?;
    push_root_doc_update(client, workspace_id, &new_state).await?;
    Ok(())
}

pub async fn unassign_tag(
    client: &AffineClient,
    workspace_id: &str,
    doc_id: &str,
    tag_id: &str,
) -> Result<()> {
    let b64 = load_root_doc(client, workspace_id).await?;
    let scripts = scripts_dir()?;
    let new_state = yjs_engine::unassign_tag_op(&scripts, &b64, doc_id, tag_id)?;
    push_root_doc_update(client, workspace_id, &new_state).await?;
    Ok(())
}

pub async fn delete_tag(
    client: &AffineClient,
    workspace_id: &str,
    tag_id: &str,
) -> Result<()> {
    let b64 = load_root_doc(client, workspace_id).await?;
    let scripts = scripts_dir()?;
    let new_state = yjs_engine::delete_tag_op(&scripts, &b64, tag_id)?;
    push_root_doc_update(client, workspace_id, &new_state).await?;
    Ok(())
}

// ── Socket.IO helpers ──────────────────────────────────────────────────────

async fn load_root_doc(client: &AffineClient, workspace_id: &str) -> Result<String> {
    let socket = connect_to_sync(client).await?;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let join_payload = json!({
        "spaceType": "workspace",
        "spaceId": workspace_id,
        "clientVersion": client.client_version(),
    });
    let _join_ack = send_with_ack(&socket, "space:join", join_payload, JOIN_TIMEOUT).await?;

    let load_payload = json!({
        "spaceType": "workspace",
        "spaceId": workspace_id,
        "docId": workspace_id,
    });
    let response = send_with_ack(&socket, "space:load-doc", load_payload, LOAD_TIMEOUT).await?;

    let b64 = extract_doc_base64(&response)?;

    let leave_payload = json!({
        "spaceType": "workspace",
        "spaceId": workspace_id,
    });
    let _ = socket.emit("space:leave", leave_payload).await;
    let _ = socket.disconnect().await;

    Ok(b64)
}

async fn push_root_doc_update(
    client: &AffineClient,
    workspace_id: &str,
    state_b64: &str,
) -> Result<()> {
    let socket = connect_to_sync(client).await?;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let join_payload = json!({
        "spaceType": "workspace",
        "spaceId": workspace_id,
        "clientVersion": client.client_version(),
    });
    let _join_ack = send_with_ack(&socket, "space:join", join_payload, JOIN_TIMEOUT).await?;

    let push_payload = json!({
        "spaceType": "workspace",
        "spaceId": workspace_id,
        "docId": workspace_id,
        "update": state_b64,
    });
    let _push_ack = send_with_ack(&socket, "space:push-doc-update", push_payload, LOAD_TIMEOUT).await?;

    let leave_payload = json!({
        "spaceType": "workspace",
        "spaceId": workspace_id,
    });
    let _ = socket.emit("space:leave", leave_payload).await;
    let _ = socket.disconnect().await;

    Ok(())
}

fn extract_doc_base64(response: &Value) -> Result<String> {
    if let Some(b64) = response.get("missing").and_then(|v| v.as_str()) {
        if !b64.is_empty() {
            if let Ok(dir) = std::env::var("YJS_DUMP_DIR") {
                let p = std::path::Path::new(&dir).join("yjs_missing_b64.txt");
                let _ = std::fs::write(&p, b64);
            }
            return Ok(b64.to_owned());
        }
    }

    if let Some(b64) = response.get("state").and_then(|v| v.as_str()) {
        if !b64.is_empty() {
            return Ok(b64.to_owned());
        }
    }

    bail!(
        "no doc binary found in response: {}",
        serde_json::to_string(&response).unwrap_or_default()
    )
}

fn socket_io_url(base_url: &str) -> Result<String> {
    let url = Url::parse(base_url)
        .with_context(|| format!("invalid base URL {base_url}"))?;
    Ok(url.origin().ascii_serialization())
}

async fn connect_to_sync(client: &AffineClient) -> Result<Client> {
    let mut builder = ClientBuilder::new(socket_io_url(client.base_url())?)
        .transport_type(TransportType::Websocket)
        .opening_header("x-affine-client-version", client.client_version());

    for (name, value) in client.auth_headers() {
        builder = builder.opening_header(name, value);
    }

    builder
        .connect()
        .await
        .context("failed to connect to AFFiNE sync server")
}

async fn send_with_ack(
    socket: &Client,
    event: &str,
    payload: Value,
    wait: Duration,
) -> Result<Value> {
    let (tx, rx) = oneshot::channel::<Value>();
    let tx = Arc::new(Mutex::new(Some(tx)));

    socket
        .emit_with_ack(
            event.to_owned(),
            payload,
            wait,
            move |result: Payload, _socket: Client| {
                let value = payload_to_json(&result);
                let tx = Arc::clone(&tx);
                Box::pin(async move {
                    if let Some(sender) = tx.lock().ok().and_then(|mut slot| slot.take()) {
                        let _ = sender.send(value);
                    }
                })
            },
        )
        .await
        .with_context(|| format!("failed to emit {event}"))?;

    let value = timeout(wait + Duration::from_secs(2), rx)
        .await
        .map_err(|_| anyhow!("timed out waiting for ack on {event}"))?
        .map_err(|_| anyhow!("ack channel dropped for {event}"))?;

    if let Some(error) = value.get("error") {
        if !error.is_null() {
            bail!("{event} failed: {error}");
        }
    }

    let value = if let Some(arr) = value.as_array() {
        if arr.len() == 1 {
            arr[0].clone()
        } else {
            return Ok(value);
        }
    } else {
        value
    };

    Ok(value.get("data").cloned().unwrap_or(value))
}

fn payload_to_json(payload: &Payload) -> Value {
    match payload {
        Payload::Text(values) => {
            if values.len() == 1 {
                values[0].clone()
            } else {
                Value::Array(values.clone())
            }
        }
        Payload::Binary(bytes) => json!({ "binary_len": bytes.len() }),
        #[allow(deprecated)]
        Payload::String(s) => serde_json::from_str::<Value>(s).unwrap_or(Value::String(s.clone())),
    }
}
