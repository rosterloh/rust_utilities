// Tag management via the AFFiNE sync protocol.
//
// Tags are stored in the workspace root Yjs document's `meta` map:
//
//   meta.properties.tags.options = [
//     { id: string, value: string, color: string, createDate: number, updateDate: number }
//   ]
//   meta.pages[pageId].tags = [tagId1, tagId2, ...]
//
// The sync protocol uses Socket.IO. After `space:join` we call `doc:load` with
// the workspace ID as the doc ID to fetch the root Yjs document.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use rust_socketio::asynchronous::{Client, ClientBuilder};
use rust_socketio::{Payload, TransportType};
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio::time::timeout;
use url::Url;
use yrs::updates::decoder::Decode;
use yrs::types::ToJson;
use yrs::{Array, Doc, Map, ReadTxn, Transact, Update};

use crate::client::AffineClient;

const LOAD_TIMEOUT: Duration = Duration::from_secs(30);
const JOIN_TIMEOUT: Duration = Duration::from_secs(30);

/// A tag option as stored in the workspace root doc.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TagOption {
    pub id: String,
    pub value: String,
    pub color: String,
    pub create_date: Option<f64>,
    pub update_date: Option<f64>,
}

/// List all tags defined in the workspace.
pub async fn list_tags(client: &AffineClient, workspace_id: &str) -> Result<Vec<TagOption>> {
    let binary = load_root_doc(client, workspace_id).await?;
    extract_tags(&binary)
}

/// Create a new tag in the workspace.
pub async fn create_tag(
    client: &AffineClient,
    workspace_id: &str,
    name: &str,
    color: &str,
) -> Result<TagOption> {
    let binary = load_root_doc(client, workspace_id).await?;
    let (update, new_tag) = mutate_tags(&binary, |tags| {
        let new_id = nanoid();
        let now = timestamp_now();
        let tag = TagOption {
            id: new_id,
            value: name.to_owned(),
            color: color.to_owned(),
            create_date: Some(now),
            update_date: Some(now),
        };
        tags.push(json!(tag));
        tag
    })?;
    push_root_doc_update(client, workspace_id, &update).await?;
    Ok(new_tag)
}

/// Assign a tag to a doc (page).
pub async fn assign_tag(
    client: &AffineClient,
    workspace_id: &str,
    doc_id: &str,
    tag_id: &str,
) -> Result<()> {
    let binary = load_root_doc(client, workspace_id).await?;
    let update = assign_tag_to_doc(&binary, doc_id, tag_id)?;
    push_root_doc_update(client, workspace_id, &update).await?;
    Ok(())
}

/// Remove a tag from a doc (page).
pub async fn unassign_tag(
    client: &AffineClient,
    workspace_id: &str,
    doc_id: &str,
    tag_id: &str,
) -> Result<()> {
    let binary = load_root_doc(client, workspace_id).await?;
    let update = unassign_tag_from_doc(&binary, doc_id, tag_id)?;
    push_root_doc_update(client, workspace_id, &update).await?;
    Ok(())
}

/// Delete a tag entirely (removes from all docs).
pub async fn delete_tag(
    client: &AffineClient,
    workspace_id: &str,
    tag_id: &str,
) -> Result<()> {
    let binary = load_root_doc(client, workspace_id).await?;
    let update = delete_tag_from_workspace(&binary, tag_id)?;
    push_root_doc_update(client, workspace_id, &update).await?;
    Ok(())
}

// ── Internal ──────────────────────────────────────────────────────────────

/// Generate a nanoid-style random ID (shorter than a UUID).
fn nanoid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut buf = [0u8; 9];
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (ts >> (i * 7)) as u8 ^ rand_byte();
    }
    base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        buf,
    )
}

fn rand_byte() -> u8 {
    use std::time::{SystemTime, UNIX_EPOCH};
    static mut SEED: u64 = 0;
    unsafe {
        if SEED == 0 {
            let ns = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            SEED = (ns & 0xFFFF_FFFF_FFFF_FFFF) as u64;
        }
        SEED = SEED.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (SEED >> 33) as u8
    }
}

fn timestamp_now() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as f64
}

/// Load the workspace root Yjs document as a binary blob.
async fn load_root_doc(client: &AffineClient, workspace_id: &str) -> Result<Vec<u8>> {
    let socket = connect_to_sync(client).await?;

    // Join the workspace space
    let join_payload = json!({
        "spaceType": "workspace",
        "spaceId": workspace_id,
        "clientVersion": client.client_version(),
    });
    let _join_ack = send_with_ack(&socket, "space:join", join_payload, JOIN_TIMEOUT).await?;

    // Load the root doc (workspace ID IS the root doc ID)
    let load_payload = json!({
        "spaceType": "workspace",
        "spaceId": workspace_id,
        "docId": workspace_id,
    });
    let response = send_with_ack(&socket, "doc:load", load_payload, LOAD_TIMEOUT).await?;

    // Extract the doc binary from the response
    let binary = extract_doc_binary(&response)?;

    // Leave and disconnect
    let leave_payload = json!({
        "spaceType": "workspace",
        "spaceId": workspace_id,
    });
    let _ = socket.emit("space:leave", leave_payload).await;
    let _ = socket.disconnect().await;

    Ok(binary)
}

/// Push a Yjs update to the workspace root doc.
async fn push_root_doc_update(
    client: &AffineClient,
    workspace_id: &str,
    update: &[u8],
) -> Result<()> {
    let socket = connect_to_sync(client).await?;

    let join_payload = json!({
        "spaceType": "workspace",
        "spaceId": workspace_id,
        "clientVersion": client.client_version(),
    });
    let _join_ack = send_with_ack(&socket, "space:join", join_payload, JOIN_TIMEOUT).await?;

    let encoded = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        update,
    );
    let push_payload = json!({
        "spaceType": "workspace",
        "spaceId": workspace_id,
        "docId": workspace_id,
        "update": encoded,
    });
    let _push_ack = send_with_ack(&socket, "doc:push-update", push_payload, LOAD_TIMEOUT).await?;

    let leave_payload = json!({
        "spaceType": "workspace",
        "spaceId": workspace_id,
    });
    let _ = socket.emit("space:leave", leave_payload).await;
    let _ = socket.disconnect().await;

    Ok(())
}

/// Extract the Yjs document binary from a `doc:load` response.
fn extract_doc_binary(response: &Value) -> Result<Vec<u8>> {
    // The AFFiNE server returns the doc data in different shapes depending on version.
    // Try multiple formats:

    // Format 1: { data: { doc: { bin: "<base64>" } } }
    if let Some(bin_str) = response
        .get("data")
        .and_then(|d| d.get("doc"))
        .and_then(|d| d.get("bin"))
        .and_then(|b| b.as_str())
    {
        return base64::Engine::decode(&base64::engine::general_purpose::STANDARD, bin_str)
            .context("failed to decode base64 doc binary");
    }

    // Format 2: { doc: { bin: "<base64>" } }
    if let Some(bin_str) = response
        .get("doc")
        .and_then(|d| d.get("bin"))
        .and_then(|b| b.as_str())
    {
        return base64::Engine::decode(&base64::engine::general_purpose::STANDARD, bin_str)
            .context("failed to decode base64 doc binary");
    }

    // Format 3: { data: "<base64>" }
    if let Some(bin_str) = response.get("data").and_then(|d| d.as_str()) {
        return base64::Engine::decode(&base64::engine::general_purpose::STANDARD, bin_str)
            .context("failed to decode base64 doc binary");
    }

    // Format 4: The response itself is a base64 string
    if let Some(bin_str) = response.as_str() {
        return base64::Engine::decode(&base64::engine::general_purpose::STANDARD, bin_str)
            .context("failed to decode base64 doc binary");
    }

    bail!(
        "unexpected doc:load response format: {}",
        serde_json::to_string(&response).unwrap_or_default()
    )
}

/// Convert a yrs::Any to a serde_json::Value.
fn any_to_json_value(any: &yrs::Any) -> Value {
    match any {
        yrs::Any::Null => Value::Null,
        yrs::Any::Undefined => Value::Null,
        yrs::Any::Bool(b) => Value::Bool(*b),
        yrs::Any::Number(n) => json!(*n),
        yrs::Any::BigInt(n) => json!(n),
        yrs::Any::String(s) => Value::String(s.to_string()),
        yrs::Any::Buffer(b) => Value::Array(b.iter().map(|x| Value::Number((*x).into())).collect()),
        yrs::Any::Array(arr) => Value::Array(arr.iter().map(any_to_json_value).collect()),
        yrs::Any::Map(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map.iter() {
                obj.insert(k.clone(), any_to_json_value(v));
            }
            Value::Object(obj)
        }
    }
}

/// Parse Yjs binary and extract tag options from meta.properties.tags.options.
fn extract_tags(binary: &[u8]) -> Result<Vec<TagOption>> {
    let doc = Doc::new();
    let mut txn = doc.transact_mut();
    txn.apply_update(Update::decode_v1(binary).context("failed to decode Yjs document")?)?;
    drop(txn);

    let txn = doc.transact();
    let meta = doc.get_or_insert_map("meta");

    // Navigate: meta -> properties -> tags -> options
    let properties = match meta.get(&txn, "properties") {
        Some(out) => out,
        None => return Ok(Vec::new()),
    };

    let tags_map = match properties.cast::<yrs::MapRef>() {
        Ok(map) => map,
        Err(_) => return Ok(Vec::new()),
    };

    let tags_value = match tags_map.get(&txn, "tags") {
        Some(out) => out,
        None => return Ok(Vec::new()),
    };

    let tags_inner = match tags_value.cast::<yrs::MapRef>() {
        Ok(map) => map,
        Err(_) => return Ok(Vec::new()),
    };

    let options = match tags_inner.get(&txn, "options") {
        Some(out) => out,
        None => return Ok(Vec::new()),
    };

    let arr = match options.cast::<yrs::ArrayRef>() {
        Ok(arr) => arr,
        Err(_) => return Ok(Vec::new()),
    };

    let mut result = Vec::new();
    for item in arr.iter(&txn) {
        let json_val = any_to_json_value(&item.to_json(&txn));
        if let Some(obj_map) = json_val.as_object() {
            let tag = TagOption {
                id: obj_map.get("id").and_then(Value::as_str).unwrap_or("").to_owned(),
                value: obj_map.get("value").and_then(Value::as_str).unwrap_or("").to_owned(),
                color: obj_map.get("color").and_then(Value::as_str).unwrap_or("").to_owned(),
                create_date: obj_map.get("createDate").or_else(|| obj_map.get("create_date")).and_then(Value::as_f64),
                update_date: obj_map.get("updateDate").or_else(|| obj_map.get("update_date")).and_then(Value::as_f64),
            };
            if !tag.id.is_empty() {
                result.push(tag);
            }
        }
    }

    Ok(result)
}

/// Convert a Vec<TagOption> to a Vec<serde_json::Value> for Yjs storage.
fn tags_to_yjs_values(tags: &[TagOption]) -> Vec<Value> {
    tags.iter()
        .map(|t| {
            json!({
                "id": t.id,
                "value": t.value,
                "color": t.color,
                "createDate": t.create_date,
                "updateDate": t.update_date,
            })
        })
        .collect()
}

/// Convert a serde_json::Value to a yrs::Any, recursing into arrays and objects.
fn json_to_yrs_any(value: &Value) -> yrs::Any {
    match value {
        Value::Null => yrs::Any::Null,
        Value::Bool(b) => yrs::Any::Bool(*b),
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                yrs::Any::Number(f.into())
            } else {
                yrs::Any::Null
            }
        }
        Value::String(s) => yrs::Any::String(s.as_str().into()),
        Value::Array(arr) => {
            yrs::Any::Array(arr.iter().map(json_to_yrs_any).collect())
        }
        Value::Object(obj) => {
            let mut map = std::collections::HashMap::new();
            for (k, v) in obj {
                map.insert(k.clone(), json_to_yrs_any(v));
            }
            yrs::Any::Map(map.into())
        }
    }
}

/// Modify tags.options in the Yjs document, returning a Yjs update.
fn mutate_tags<F, T>(binary: &[u8], f: F) -> Result<(Vec<u8>, T)>
where
    F: FnOnce(&mut Vec<Value>) -> T,
{
    let doc = Doc::new();
    let mut txn = doc.transact_mut();
    txn.apply_update(Update::decode_v1(binary).context("failed to decode Yjs document")?)?;
    drop(txn);

    // Read current tags
    let current_tags = extract_tags(binary)?;

    let mut tags_json = tags_to_yjs_values(&current_tags);
    let result = f(&mut tags_json);

    // Write back options array
    {
        let mut txn = doc.transact_mut();
        // Build the nested structure: meta -> properties -> tags -> options
        let meta = doc.get_or_insert_map("meta");
        let properties = meta.get_or_init::<_, yrs::MapRef>(&mut txn, "properties");
        let tags_map = properties.get_or_init::<_, yrs::MapRef>(&mut txn, "tags");

        // Convert tag values to yrs::Any array
        let any_array: Vec<yrs::Any> = tags_json.iter().map(json_to_yrs_any).collect();
        let array_as_any = yrs::Any::Array(any_array.into());
        tags_map.insert(&mut txn, "options", array_as_any);
    }

    let update = doc.transact_mut().encode_state_as_update_v1(&yrs::StateVector::default());
    Ok((update, result))
}

/// Add a tag ID to a doc's tags array.
fn assign_tag_to_doc(binary: &[u8], doc_id: &str, tag_id: &str) -> Result<Vec<u8>> {
    let doc = Doc::new();
    let mut txn = doc.transact_mut();
    txn.apply_update(Update::decode_v1(binary).context("failed to decode Yjs document")?)?;
    drop(txn);

    let meta = doc.get_or_insert_map("meta");
    let mut txn = doc.transact_mut();
    let pages = meta.get_or_init::<_, yrs::MapRef>(&mut txn, "pages");
    let doc_entry = pages.get_or_init::<_, yrs::MapRef>(&mut txn, doc_id);

    // Read current tags (snapshot them to avoid borrow conflicts)
    let has_tag = doc_entry.get(&txn, "tags").map_or(false, |out| {
        out.cast::<yrs::ArrayRef>().map_or(false, |arr| {
            arr.iter(&txn).any(|a| {
                matches!(a, yrs::Out::Any(yrs::Any::String(s)) if s.as_ref() == tag_id)
            })
        })
    });

    if !has_tag {
        // Snapshot existing tag IDs
        let mut existing: Vec<String> = doc_entry.get(&txn, "tags").map_or(Vec::new(), |out| {
            out.cast::<yrs::ArrayRef>().map_or(Vec::new(), |arr| {
                arr.iter(&txn)
                    .filter_map(|a| match a {
                        yrs::Out::Any(yrs::Any::String(s)) => Some(s.to_string()),
                        _ => None,
                    })
                    .collect()
            })
        });
        existing.push(tag_id.to_owned());

        let any_array: Vec<yrs::Any> = existing
            .iter()
            .map(|s| yrs::Any::String(s.as_str().into()))
            .collect();
        doc_entry.insert(&mut txn, "tags", yrs::Any::Array(any_array.into()));
    }

    Ok(txn.encode_state_as_update_v1(&yrs::StateVector::default()))
}

/// Remove a tag ID from a doc's tags array.
fn unassign_tag_from_doc(binary: &[u8], doc_id: &str, tag_id: &str) -> Result<Vec<u8>> {
    let doc = Doc::new();
    let mut txn = doc.transact_mut();
    txn.apply_update(Update::decode_v1(binary).context("failed to decode Yjs document")?)?;
    drop(txn);

    let meta = doc.get_or_insert_map("meta");
    let mut txn = doc.transact_mut();
    let pages = meta.get_or_init::<_, yrs::MapRef>(&mut txn, "pages");
    let doc_entry = pages.get_or_init::<_, yrs::MapRef>(&mut txn, doc_id);

    let mut current_tags: Vec<String> = match doc_entry.get(&txn, "tags") {
        Some(out) => {
            if let Ok(arr) = out.cast::<yrs::ArrayRef>() {
                arr.iter(&txn)
                    .filter_map(|a| match a {
                        yrs::Out::Any(yrs::Any::String(s)) => Some(s.to_string()),
                        _ => None,
                    })
                    .collect()
            } else {
                Vec::new()
            }
        }
        None => Vec::new(),
    };

    current_tags.retain(|t| t != tag_id);
    let any_array: Vec<yrs::Any> = current_tags
        .iter()
        .map(|s| yrs::Any::String(s.as_str().into()))
        .collect();
    doc_entry.insert(&mut txn, "tags", yrs::Any::Array(any_array.into()));

    Ok(txn.encode_state_as_update_v1(&yrs::StateVector::default()))
}

/// Delete a tag from workspace (remove from options + all docs).
fn delete_tag_from_workspace(binary: &[u8], tag_id: &str) -> Result<Vec<u8>> {
    // First, remove from options
    let (update, _) = mutate_tags(binary, |tags| {
        tags.retain(|t| t.get("id").and_then(Value::as_str) != Some(tag_id));
    })?;

    // Then apply update and also remove from all docs
    let doc = Doc::new();
    let mut txn = doc.transact_mut();
    txn.apply_update(Update::decode_v1(&update).context("failed to decode Yjs update")?)?;
    drop(txn);

    let meta = doc.get_or_insert_map("meta");
    let txn = doc.transact_mut();
    if let Some(out) = meta.get(&txn, "pages") {
        if let Ok(pages) = out.cast::<yrs::MapRef>() {
            // Collect page IDs first to avoid borrow issues
            let page_ids: Vec<String> = pages
                .keys(&txn)
                .map(|k| k.to_string())
                .collect();
            for page_id in &page_ids {
                let binary_current = txn.encode_state_as_update_v1(&yrs::StateVector::default());
                let doc_update = unassign_tag_from_doc(&binary_current, page_id.as_str(), tag_id)?;
                let mut txn2 = doc.transact_mut();
                txn2.apply_update(Update::decode_v1(&doc_update)?)?;
                drop(txn2);
            }
        }
    }

    let final_update = doc.transact().encode_state_as_update_v1(&yrs::StateVector::default());
    Ok(final_update)
}

// ── Socket.IO helpers ─────────────────────────────────────────────────────

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
