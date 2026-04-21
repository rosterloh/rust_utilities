use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use reqwest::header::{AUTHORIZATION, COOKIE, HeaderMap, HeaderValue, SET_COOKIE};
use reqwest::multipart::{Form, Part};
use reqwest::{Client, Method, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub enum AuthState {
    None,
    Bearer(String),
    Cookies(BTreeMap<String, String>),
}

#[derive(Debug)]
pub struct SignInResponse {
    pub body: Value,
    pub cookies: BTreeMap<String, String>,
}

#[derive(Debug)]
pub struct BlobDownload {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
}

#[derive(Debug)]
pub struct BlobHead {
    pub status: StatusCode,
    pub content_length: Option<u64>,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Debug)]
pub struct PresignedPutOutcome {
    pub status: StatusCode,
    pub etag: Option<String>,
}

pub struct AffineClient {
    http: Client,
    base_url: String,
    client_version: String,
    auth: AuthState,
}

impl AffineClient {
    pub fn new(
        base_url: impl Into<String>,
        client_version: impl Into<String>,
        auth: AuthState,
    ) -> Result<Self> {
        let base_url = normalize_base_url(&base_url.into());
        let http = Client::builder()
            .user_agent("affine-cli/0.1.0")
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self {
            http,
            base_url,
            client_version: client_version.into(),
            auth,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn sign_in(
        &self,
        email: &str,
        password: Option<&str>,
        callback_url: Option<&str>,
    ) -> Result<SignInResponse> {
        let mut payload = serde_json::Map::new();
        payload.insert("email".to_owned(), Value::String(email.to_owned()));

        if let Some(password) = password {
            payload.insert("password".to_owned(), Value::String(password.to_owned()));
        }

        if let Some(callback_url) = callback_url {
            payload.insert(
                "callbackUrl".to_owned(),
                Value::String(callback_url.to_owned()),
            );
        }

        let response = self
            .base_request(Method::POST, &self.join_path("/api/auth/sign-in"))?
            .json(&Value::Object(payload))
            .send()
            .await
            .context("failed to send sign-in request")?;

        let status = response.status();
        let headers = response.headers().clone();
        let body = response
            .json::<Value>()
            .await
            .context("failed to decode sign-in response body")?;

        if !status.is_success() {
            return Err(http_json_error(status, &body));
        }

        let mut cookies = BTreeMap::new();
        merge_set_cookie_headers(&mut cookies, &headers)?;

        Ok(SignInResponse { body, cookies })
    }

    pub async fn sign_out(
        &self,
        existing_cookies: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, String>> {
        let mut request = self.base_request(Method::POST, &self.join_path("/api/auth/sign-out"))?;
        request = self.apply_auth(request);

        if let Some(csrf) = existing_cookies.get("affine_csrf_token") {
            request = request.header("x-affine-csrf-token", csrf);
        }

        let response = request
            .send()
            .await
            .context("failed to send sign-out request")?;

        let status = response.status();
        let headers = response.headers().clone();
        let body = response.json::<Value>().await.unwrap_or_else(|_| json!({}));

        if !status.is_success() {
            return Err(http_json_error(status, &body));
        }

        let mut cookies = existing_cookies.clone();
        merge_set_cookie_headers(&mut cookies, &headers)?;
        Ok(cookies)
    }

    pub async fn graphql(
        &self,
        query: &str,
        operation_name: Option<&str>,
        variables: Value,
    ) -> Result<Value> {
        let mut body = serde_json::Map::new();
        body.insert("query".to_owned(), Value::String(query.to_owned()));
        body.insert("variables".to_owned(), variables);

        if let Some(operation_name) = operation_name {
            body.insert(
                "operationName".to_owned(),
                Value::String(operation_name.to_owned()),
            );
        }

        let response = self
            .apply_auth(self.base_request(Method::POST, &self.join_path("/graphql"))?)
            .json(&Value::Object(body))
            .send()
            .await
            .context("failed to send GraphQL request")?;

        Self::graphql_response(response).await
    }

    pub async fn graphql_upload(
        &self,
        query: &str,
        operation_name: &str,
        variables: Value,
        file_variable: &str,
        file_path: &Path,
    ) -> Result<Value> {
        let file_name = file_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow!("invalid upload file name: {}", file_path.display()))?;

        let bytes = tokio::fs::read(file_path)
            .await
            .with_context(|| format!("failed to read upload file {}", file_path.display()))?;

        let mut mapped_variables = variables;
        let target = mapped_variables
            .as_object_mut()
            .ok_or_else(|| anyhow!("upload variables must be a JSON object"))?;
        target.insert(file_variable.to_owned(), Value::Null);

        let operations = json!({
            "query": query,
            "operationName": operation_name,
            "variables": mapped_variables,
        });
        let map = json!({
            "0": [format!("variables.{file_variable}")]
        });

        let mime = mime_guess::from_path(file_path)
            .first_or_octet_stream()
            .to_string();

        let part = Part::bytes(bytes)
            .file_name(file_name.to_owned())
            .mime_str(&mime)
            .with_context(|| {
                format!("failed to set upload MIME type for {}", file_path.display())
            })?;

        let form = Form::new()
            .text("operations", operations.to_string())
            .text("map", map.to_string())
            .part("0", part);

        let response = self
            .apply_auth(self.base_request(Method::POST, &self.join_path("/graphql"))?)
            .multipart(form)
            .send()
            .await
            .context("failed to send GraphQL upload request")?;

        Self::graphql_response(response).await
    }

    pub async fn download_blob(&self, workspace_id: &str, key: &str) -> Result<BlobDownload> {
        let response = self
            .apply_auth(self.base_request(
                Method::GET,
                &self.join_path(&format!("/api/workspaces/{workspace_id}/blobs/{key}")),
            )?)
            .send()
            .await
            .context("failed to download blob")?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .json::<Value>()
                .await
                .unwrap_or_else(|_| json!({"message": "blob download failed"}));
            return Err(http_json_error(status, &body));
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.to_owned());
        let bytes = response
            .bytes()
            .await
            .context("failed to read blob response body")?;

        Ok(BlobDownload {
            bytes: bytes.to_vec(),
            content_type,
        })
    }

    pub async fn head_blob(&self, workspace_id: &str, key: &str) -> Result<BlobHead> {
        let response = self
            .apply_auth(self.base_request(
                Method::HEAD,
                &self.join_path(&format!("/api/workspaces/{workspace_id}/blobs/{key}")),
            )?)
            .send()
            .await
            .context("failed to HEAD blob")?;

        let status = response.status();
        if !status.is_success() && status != StatusCode::NOT_FOUND {
            bail!("HTTP {} on HEAD blob {}", status.as_u16(), key);
        }

        let headers = response.headers();
        let content_length = headers
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());
        let content_type = headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let etag = headers
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let last_modified = headers
            .get(reqwest::header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);

        Ok(BlobHead {
            status,
            content_length,
            content_type,
            etag,
            last_modified,
        })
    }

    /// Raw REST call against the AFFiNE server. JSON body optional, returns parsed JSON
    /// when the response advertises JSON — otherwise an empty object.
    pub async fn rest_json(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let mut request = self.apply_auth(self.base_request(method, &self.join_path(path))?);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .with_context(|| format!("failed to call {path}"))?;

        let status = response.status();
        let parsed = response.json::<Value>().await.unwrap_or_else(|_| json!({}));

        if !status.is_success() {
            return Err(http_json_error(status, &parsed));
        }

        Ok(parsed)
    }

    /// PUT arbitrary bytes to an externally-provided (presigned) URL, attaching any
    /// extra headers that the server asked the client to include.
    pub async fn put_presigned(
        &self,
        url: &str,
        bytes: Vec<u8>,
        headers: Option<&serde_json::Map<String, Value>>,
    ) -> Result<PresignedPutOutcome> {
        let mut builder = self
            .http
            .request(Method::PUT, url)
            .body(bytes);

        if let Some(headers) = headers {
            for (name, value) in headers {
                if let Some(s) = value.as_str() {
                    builder = builder.header(name, s);
                }
            }
        }

        let response = builder
            .send()
            .await
            .context("failed to PUT to presigned URL")?;

        let status = response.status();
        let etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            bail!(
                "HTTP {} from presigned PUT: {}",
                status.as_u16(),
                body.chars().take(400).collect::<String>()
            );
        }

        Ok(PresignedPutOutcome { status, etag })
    }

    /// Invoke magic-link OTP exchange. The body should mirror the JSON expected by
    /// `/api/auth/magic-link` (typically `{ email, token, callbackUrl? }`). We also
    /// capture any Set-Cookie headers so the session can be persisted.
    pub async fn magic_link(
        &self,
        email: &str,
        token: &str,
        callback_url: Option<&str>,
    ) -> Result<SignInResponse> {
        let mut payload = serde_json::Map::new();
        payload.insert("email".to_owned(), Value::String(email.to_owned()));
        payload.insert("token".to_owned(), Value::String(token.to_owned()));
        if let Some(callback_url) = callback_url {
            payload.insert(
                "callbackUrl".to_owned(),
                Value::String(callback_url.to_owned()),
            );
        }

        let response = self
            .apply_auth(self.base_request(Method::POST, &self.join_path("/api/auth/magic-link"))?)
            .json(&Value::Object(payload))
            .send()
            .await
            .context("failed to send magic-link request")?;

        let status = response.status();
        let headers = response.headers().clone();
        let body = response.json::<Value>().await.unwrap_or_else(|_| json!({}));

        if !status.is_success() {
            return Err(http_json_error(status, &body));
        }

        let mut cookies = BTreeMap::new();
        merge_set_cookie_headers(&mut cookies, &headers)?;

        Ok(SignInResponse { body, cookies })
    }

    fn base_request(&self, method: Method, url: &str) -> Result<reqwest::RequestBuilder> {
        let version = HeaderValue::from_str(&self.client_version).with_context(|| {
            format!(
                "invalid client version header value {}",
                self.client_version
            )
        })?;

        Ok(self
            .http
            .request(method, url)
            .header("x-affine-client-version", version))
    }

    fn apply_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.auth {
            AuthState::None => request,
            AuthState::Bearer(token) => request.header(AUTHORIZATION, format!("Bearer {token}")),
            AuthState::Cookies(cookies) => {
                if cookies.is_empty() {
                    request
                } else {
                    request.header(COOKIE, cookie_header_value(cookies))
                }
            }
        }
    }

    /// Build a `(name, value)` list of auth headers for use with non-reqwest clients
    /// (for example the Socket.IO client in [`crate::sync`]).
    pub fn auth_headers(&self) -> Vec<(String, String)> {
        match &self.auth {
            AuthState::None => Vec::new(),
            AuthState::Bearer(token) => {
                vec![("Authorization".to_owned(), format!("Bearer {token}"))]
            }
            AuthState::Cookies(cookies) => {
                if cookies.is_empty() {
                    Vec::new()
                } else {
                    vec![("Cookie".to_owned(), cookie_header_value(cookies))]
                }
            }
        }
    }

    pub fn client_version(&self) -> &str {
        &self.client_version
    }

    fn join_path(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn graphql_response(response: reqwest::Response) -> Result<Value> {
        let status = response.status();
        let envelope = response
            .json::<GraphQlEnvelope>()
            .await
            .context("failed to decode GraphQL response")?;

        if !status.is_success() {
            let errors = envelope
                .errors
                .as_ref()
                .map(|errors| format_graphql_errors(errors))
                .unwrap_or_else(|| "request failed".to_owned());
            bail!("GraphQL HTTP error {}: {errors}", status.as_u16());
        }

        if let Some(errors) = envelope.errors {
            bail!("{}", format_graphql_errors(&errors));
        }

        envelope
            .data
            .ok_or_else(|| anyhow!("GraphQL response did not include data"))
    }
}

#[derive(Debug, Deserialize)]
struct GraphQlEnvelope {
    data: Option<Value>,
    errors: Option<Vec<GraphQlError>>,
}

#[derive(Debug, Deserialize)]
struct GraphQlError {
    message: String,
    #[serde(default)]
    extensions: Option<GraphQlExtensions>,
}

#[derive(Debug, Deserialize)]
struct GraphQlExtensions {
    #[serde(default)]
    code: Option<String>,
}

fn format_graphql_errors(errors: &[GraphQlError]) -> String {
    errors
        .iter()
        .map(|error| {
            if let Some(code) = error.extensions.as_ref().and_then(|ext| ext.code.as_ref()) {
                format!("{} ({code})", error.message)
            } else {
                error.message.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn cookie_header_value(cookies: &BTreeMap<String, String>) -> String {
    cookies
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn merge_set_cookie_headers(
    cookies: &mut BTreeMap<String, String>,
    headers: &HeaderMap,
) -> Result<()> {
    for value in headers.get_all(SET_COOKIE) {
        let value = value
            .to_str()
            .context("received non-UTF-8 Set-Cookie header")?;
        let cookie_pair = value.split(';').next().unwrap_or_default();

        if let Some((name, raw_value)) = cookie_pair.split_once('=') {
            if raw_value.is_empty() {
                cookies.remove(name);
            } else {
                cookies.insert(name.to_owned(), raw_value.to_owned());
            }
        }
    }

    Ok(())
}

fn http_json_error(status: StatusCode, body: &Value) -> anyhow::Error {
    let message = body
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("request failed");
    let code = body.get("name").and_then(Value::as_str).unwrap_or("");

    if code.is_empty() {
        anyhow!("HTTP {}: {message}", status.as_u16())
    } else {
        anyhow!("HTTP {}: {message} ({code})", status.as_u16())
    }
}

fn normalize_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    trimmed
        .strip_suffix("/graphql")
        .unwrap_or(trimmed)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{cookie_header_value, normalize_base_url};

    #[test]
    fn normalizes_graphql_endpoint_to_base_url() {
        assert_eq!(
            normalize_base_url("https://app.affine.pro/graphql"),
            "https://app.affine.pro"
        );
        assert_eq!(
            normalize_base_url("https://app.affine.pro/"),
            "https://app.affine.pro"
        );
    }

    #[test]
    fn formats_cookie_header() {
        let mut cookies = BTreeMap::new();
        cookies.insert("affine_csrf_token".to_owned(), "csrf".to_owned());
        cookies.insert("affine_session".to_owned(), "session".to_owned());

        assert_eq!(
            cookie_header_value(&cookies),
            "affine_csrf_token=csrf; affine_session=session"
        );
    }
}
