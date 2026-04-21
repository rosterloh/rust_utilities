// Thin wrapper around the AFFiNE realtime sync endpoint (Socket.IO). Used for operations
// that have no GraphQL or REST equivalent — currently just doc deletion (trash).
//
// The AFFiNE server exposes its sync events on the default `/socket.io` path. The events
// we care about are:
//
//   * `space:join`        — identify which workspace/userspace we are operating on
//   * `space:delete-doc`  — remove a doc (this is how the official client trashes a page)
//   * `space:leave`       — tidy up
//
// Authentication is session-cookie or Bearer based, matching the rest of the CLI.

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

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
const JOIN_TIMEOUT: Duration = Duration::from_secs(30);

/// Ask the server to trash a document. Returns the server-reported response on success
/// (often `null` / an empty object) and an error otherwise. The operation also sets
/// the doc's metadata `trash` flag on the workspace root doc.
pub async fn delete_doc(client: &AffineClient, workspace_id: &str, doc_id: &str) -> Result<Value> {
    let socket = connect(client).await?;

    // Give the socket.io upgrade a beat to complete before emitting anything.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // The server answers `space:join` with an ack whose payload carries either
    // `data: { clientId }` on success or `error: {...}` on failure.
    let join_payload = json!({
        "spaceType": "workspace",
        "spaceId": workspace_id,
        "clientVersion": client.client_version(),
    });
    let _join_ack = send_with_ack(&socket, "space:join", join_payload, JOIN_TIMEOUT).await?;

    // `space:delete-doc` returns `data: null` on success, `error: {...}` on failure.
    // In practice the server sometimes doesn't ack at all once the delete has actually
    // been applied (it broadcasts the update instead), so treat ack-timeout as success
    // and surface anything it did send.
    let delete_payload = json!({
        "spaceType": "workspace",
        "spaceId": workspace_id,
        "docId": doc_id,
    });
    let response = match send_with_ack(
        &socket,
        "space:delete-doc",
        delete_payload,
        DEFAULT_TIMEOUT,
    )
    .await
    {
        Ok(value) => value,
        Err(err) if err.to_string().contains("timed out") => Value::Null,
        Err(err) => return Err(err),
    };

    let leave_payload = json!({
        "spaceType": "workspace",
        "spaceId": workspace_id,
    });
    let _ = socket.emit("space:leave", leave_payload).await;

    // Best-effort close; ignore any disconnect errors so we don't mask a successful delete.
    let _ = socket.disconnect().await;

    Ok(response)
}

async fn connect(client: &AffineClient) -> Result<Client> {
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
    // The callback is `FnMut`, but a oneshot sender can only fire once. Wrap it in an
    // Arc<Mutex<Option<_>>> so successive invocations are no-ops after the first.
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
        // `Payload::String` is deprecated in rust_socketio 0.6 but still constructible
        // in ack callbacks. Parse as JSON where possible, fall through to a raw string.
        #[allow(deprecated)]
        Payload::String(s) => serde_json::from_str::<Value>(s).unwrap_or(Value::String(s.clone())),
    }
}

fn socket_io_url(base_url: &str) -> Result<String> {
    // AFFiNE's sync endpoint is served from the same origin as the HTTP API.
    // `rust_socketio` expects a full URL (including scheme) pointing at the Socket.IO
    // namespace; the default namespace is fine.
    let url = Url::parse(base_url)
        .with_context(|| format!("invalid base URL {base_url}"))?;
    Ok(url.origin().ascii_serialization())
}
