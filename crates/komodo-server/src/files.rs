//! Native MCP transfers for literal credential values outside model context.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    body::{Body, to_bytes},
    extract::{Path, Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use komodo_mcp::files::{CredentialFiles, FileValue};
use rmcp::{ErrorData, model::Meta};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;
use url::Url;
use zeroize::Zeroizing;

const URI_PREFIX: &str = "mcp-file://komodo/";
const HTTP_PREFIX: &str = "/file-transfers/";
const MAX_UPLOAD_BYTES: usize = 256 * 1024;
const MAX_DOWNLOAD_BYTES: usize = 2 * 1024 * 1024;
const MAX_ENTRIES: usize = 64;
const MAX_STORED_BYTES: usize = 16 * 1024 * 1024;
const FILE_TTL: Duration = Duration::from_mins(5);
const GRANT_TTL: Duration = Duration::from_mins(1);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UploadParams {
    #[serde(default, rename = "_meta")]
    _meta: BTreeMap<String, Value>,
    name: Option<String>,
    mime_type: Option<String>,
    size: Option<u64>,
    digest: Option<FileDigest>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DownloadParams {
    #[serde(default, rename = "_meta")]
    _meta: BTreeMap<String, Value>,
    uri: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDigest {
    algorithm: String,
    value: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Upload,
    Download,
}

struct Grant {
    hash: [u8; 32],
    expires: Instant,
    direction: Direction,
}

struct Entry {
    owner: String,
    file: FileValue,
    bytes: Option<Zeroizing<Vec<u8>>>,
    expected_digest: Option<[u8; 32]>,
    expected_size: Option<u64>,
    expires: Instant,
    grant: Option<Grant>,
    uploaded: bool,
}

#[derive(Default)]
pub(crate) struct TransferFiles {
    entries: Mutex<HashMap<String, Entry>>,
}

fn random_id() -> Result<String, ErrorData> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| ErrorData::internal_error("file authorization entropy unavailable", None))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn unavailable() -> ErrorData {
    ErrorData::invalid_params("credential file is unavailable for this request", None)
}

fn prune(entries: &mut HashMap<String, Entry>) {
    let now = Instant::now();
    entries.retain(|_, entry| entry.expires > now);
}

fn admit(entries: &HashMap<String, Entry>, size: usize) -> Result<(), ErrorData> {
    let stored: usize = entries
        .values()
        .filter_map(|entry| entry.bytes.as_ref())
        .map(|bytes| bytes.len())
        .sum();
    if entries.len() >= MAX_ENTRIES || stored.saturating_add(size) > MAX_STORED_BYTES {
        return Err(ErrorData::internal_error(
            "credential transfer capacity unavailable",
            None,
        ));
    }
    Ok(())
}

fn file_id(uri: &str) -> Result<&str, ErrorData> {
    let id = uri.strip_prefix(URI_PREFIX).ok_or_else(unavailable)?;
    if id.len() != 43
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(unavailable());
    }
    Ok(id)
}

fn descriptor(base: &Url, id: &str, direction: Direction) -> Result<(Grant, Value), ErrorData> {
    let token = Zeroizing::new(random_id()?);
    let grant = Grant {
        hash: Sha256::digest(token.as_bytes()).into(),
        expires: Instant::now() + GRANT_TTL,
        direction,
    };
    let url = base
        .join(&format!("{HTTP_PREFIX}{id}"))
        .map_err(|_| ErrorData::internal_error("file transfer URL unavailable", None))?;
    let descriptor = json!({
        "transport":"http", "method":if direction == Direction::Upload {"PUT"} else {"GET"},
        "url":url.as_str(), "headers":{"Authorization":format!("Bearer {}", token.as_str())}
    });
    Ok((grant, descriptor))
}

pub(crate) fn supports_http(meta: &Meta, direction: &str) -> bool {
    let files = &meta
        .get("io.modelcontextprotocol/clientCapabilities")
        .and_then(|v| v.get("files"));
    files.is_some_and(|v| {
        v.get(direction).and_then(Value::as_bool) == Some(true)
            && v.get("transports")
                .and_then(Value::as_array)
                .is_some_and(|values| values.iter().any(|v| v == "https" || v == "http"))
    })
}

impl TransferFiles {
    pub(crate) async fn expire_until_shutdown(
        &self,
        cancellation: tokio_util::sync::CancellationToken,
    ) {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                biased;
                () = cancellation.cancelled() => {
                    self.entries.lock().expect("credential transfer registry poisoned").clear();
                    return;
                }
                _ = interval.tick() => {
                    prune(&mut self.entries.lock().expect("credential transfer registry poisoned"));
                }
            }
        }
    }
    pub(crate) fn authorize_upload(
        &self,
        owner: &str,
        params: UploadParams,
        base: &Url,
    ) -> Result<Value, ErrorData> {
        if params
            .size
            .is_some_and(|size| size > MAX_UPLOAD_BYTES as u64)
            || params
                .name
                .as_ref()
                .is_some_and(|v| v.len() > 255 || v.chars().any(char::is_control))
            || params
                .mime_type
                .as_ref()
                .is_some_and(|v| v.len() > 255 || v.chars().any(char::is_control))
        {
            return Err(ErrorData::invalid_params(
                "credential upload metadata exceeds its bounds",
                None,
            ));
        }
        let expected_digest = params
            .digest
            .map(|digest| {
                if digest.algorithm != "sha-256" {
                    return Err(ErrorData::invalid_params(
                        "unsupported upload digest algorithm",
                        None,
                    ));
                }
                URL_SAFE_NO_PAD
                    .decode(digest.value)
                    .ok()
                    .and_then(|value| value.try_into().ok())
                    .ok_or_else(|| ErrorData::invalid_params("invalid upload digest", None))
            })
            .transpose()?;
        let id = random_id()?;
        let (grant, upload) = descriptor(base, &id, Direction::Upload)?;
        let file = FileValue {
            uri: format!("{URI_PREFIX}{id}"),
            name: params.name,
            mime_type: params.mime_type,
            size: params.size,
            digest: None,
        };
        let mut entries = self
            .entries
            .lock()
            .expect("credential transfer registry poisoned");
        prune(&mut entries);
        admit(&entries, 0)?;
        if entries.contains_key(&id) {
            return Err(ErrorData::internal_error("file identifier collision", None));
        }
        entries.insert(
            id,
            Entry {
                owner: owner.to_owned(),
                file: file.clone(),
                bytes: None,
                expected_digest,
                expected_size: params.size,
                expires: Instant::now() + FILE_TTL,
                grant: Some(grant),
                uploaded: true,
            },
        );
        Ok(json!({"file":file,"upload":upload}))
    }

    pub(crate) fn authorize_download(
        &self,
        owner: &str,
        params: &DownloadParams,
        base: &Url,
    ) -> Result<Value, ErrorData> {
        let id = file_id(&params.uri)?;
        let (grant, download) = descriptor(base, id, Direction::Download)?;
        let mut entries = self
            .entries
            .lock()
            .expect("credential transfer registry poisoned");
        prune(&mut entries);
        let entry = entries
            .get_mut(id)
            .filter(|entry| entry.owner == owner && entry.bytes.is_some())
            .ok_or_else(unavailable)?;
        entry.grant = Some(grant);
        Ok(json!({"file":entry.file,"download":download,"sensitivity":"secret"}))
    }

    fn upload(&self, id: &str, token: &str, bytes: Vec<u8>) -> Result<(), ErrorData> {
        let bytes = Zeroizing::new(bytes);
        let mut entries = self
            .entries
            .lock()
            .expect("credential transfer registry poisoned");
        prune(&mut entries);
        let stored: usize = entries
            .values()
            .filter_map(|entry| entry.bytes.as_ref())
            .map(|v| v.len())
            .sum();
        let entry = entries
            .get_mut(id)
            .filter(|entry| entry.bytes.is_none() && valid_grant(entry, token, Direction::Upload))
            .ok_or_else(unavailable)?;
        if bytes.len() > MAX_UPLOAD_BYTES || stored.saturating_add(bytes.len()) > MAX_STORED_BYTES {
            return Err(ErrorData::invalid_params(
                "credential upload exceeds its byte capacity",
                None,
            ));
        }
        if entry
            .expected_size
            .is_some_and(|size| size != bytes.len() as u64)
            || entry.expected_digest.is_some_and(|expected| {
                Sha256::digest(bytes.as_slice())[..]
                    .ct_eq(&expected)
                    .unwrap_u8()
                    != 1
            })
        {
            return Err(ErrorData::invalid_params(
                "credential upload integrity mismatch",
                None,
            ));
        }
        entry.file.size = Some(bytes.len() as u64);
        entry.bytes = Some(bytes);
        entry.grant = None;
        Ok(())
    }

    fn download(&self, id: &str, token: &str) -> Result<Zeroizing<Vec<u8>>, ErrorData> {
        let mut entries = self
            .entries
            .lock()
            .expect("credential transfer registry poisoned");
        prune(&mut entries);
        if !entries.get(id).is_some_and(|entry| {
            entry.bytes.is_some() && valid_grant(entry, token, Direction::Download)
        }) {
            return Err(unavailable());
        }
        entries
            .remove(id)
            .and_then(|entry| entry.bytes)
            .ok_or_else(unavailable)
    }
}

fn valid_grant(entry: &Entry, token: &str, direction: Direction) -> bool {
    entry.grant.as_ref().is_some_and(|grant| {
        grant.direction == direction
            && grant.expires > Instant::now()
            && Sha256::digest(token.as_bytes())[..]
                .ct_eq(&grant.hash)
                .unwrap_u8()
                == 1
    })
}

impl CredentialFiles for TransferFiles {
    fn publish(
        &self,
        owner: &str,
        name: &'static str,
        mime_type: &'static str,
        bytes: Zeroizing<Vec<u8>>,
    ) -> Result<FileValue, ErrorData> {
        if bytes.len() > MAX_DOWNLOAD_BYTES {
            return Err(ErrorData::internal_error(
                "credential download exceeds its byte bound",
                None,
            ));
        }
        let id = random_id()?;
        let file = FileValue {
            uri: format!("{URI_PREFIX}{id}"),
            name: Some(name.to_owned()),
            mime_type: Some(mime_type.to_owned()),
            size: Some(bytes.len() as u64),
            digest: None,
        };
        let mut entries = self
            .entries
            .lock()
            .expect("credential transfer registry poisoned");
        prune(&mut entries);
        admit(&entries, bytes.len())?;
        if entries.contains_key(&id) {
            return Err(ErrorData::internal_error("file identifier collision", None));
        }
        entries.insert(
            id,
            Entry {
                owner: owner.to_owned(),
                file: file.clone(),
                bytes: Some(bytes),
                expected_digest: None,
                expected_size: None,
                expires: Instant::now() + FILE_TTL,
                grant: None,
                uploaded: false,
            },
        );
        Ok(file)
    }

    fn take(&self, owner: &str, uri: &str) -> Result<Zeroizing<Vec<u8>>, ErrorData> {
        let id = file_id(uri)?;
        let mut entries = self
            .entries
            .lock()
            .expect("credential transfer registry poisoned");
        prune(&mut entries);
        if !entries
            .get(id)
            .is_some_and(|entry| entry.owner == owner && entry.uploaded && entry.bytes.is_some())
        {
            return Err(unavailable());
        }
        entries
            .remove(id)
            .and_then(|entry| entry.bytes)
            .ok_or_else(unavailable)
    }
}

fn bearer(request: &Request) -> Option<&str> {
    let mut values = request.headers().get_all(header::AUTHORIZATION).iter();
    let token = values.next()?.to_str().ok()?.strip_prefix("Bearer ")?;
    (values.next().is_none() && token.len() == 43).then_some(token)
}

pub(crate) async fn upload_http(
    State(files): State<Arc<TransferFiles>>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let Some(budget) = crate::lifecycle::request_budget(&request) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let Some(token) = bearer(&request).map(|v| Zeroizing::new(v.to_owned())) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let Ok(bytes) = to_bytes(request.into_body(), MAX_UPLOAD_BYTES).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    if budget.check().is_err() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    match files.upload(&id, &token, bytes.to_vec()) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::BAD_REQUEST.into_response(),
    }
}

pub(crate) async fn download_http(
    State(files): State<Arc<TransferFiles>>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    if crate::lifecycle::request_budget(&request).is_none_or(|budget| budget.check().is_err()) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let Some(token) = bearer(&request) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match files.download(&id, token) {
        Ok(mut bytes) => (
            [
                (header::CONTENT_TYPE, "application/octet-stream"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            Body::from(std::mem::take(&mut *bytes)),
        )
            .into_response(),
        Err(_) => StatusCode::BAD_REQUEST.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("http://localhost/").unwrap()
    }

    fn token(value: &Value, direction: &str) -> String {
        value[direction]["headers"]["Authorization"]
            .as_str()
            .unwrap()
            .strip_prefix("Bearer ")
            .unwrap()
            .to_owned()
    }

    #[test]
    fn completed_upload_is_owned_and_consumed_once() {
        let files = TransferFiles::default();
        let bytes = b"fixture-token";
        let params = serde_json::from_value(json!({"size":bytes.len(),"digest":{
            "algorithm":"sha-256","value":URL_SAFE_NO_PAD.encode(Sha256::digest(bytes))
        }}))
        .unwrap();
        let authorized = files.authorize_upload("alice", params, &base()).unwrap();
        let uri = authorized["file"]["uri"].as_str().unwrap();
        let id = file_id(uri).unwrap();
        assert!(files.take("alice", uri).is_err());
        assert!(files.upload(id, &"x".repeat(43), bytes.to_vec()).is_err());
        let grant = token(&authorized, "upload");
        assert!(files.upload(id, &grant, b"wrong-payload".to_vec()).is_err());
        files.upload(id, &grant, bytes.to_vec()).unwrap();
        assert!(files.upload(id, &grant, bytes.to_vec()).is_err());
        assert!(files.take("bob", uri).is_err());
        assert_eq!(files.take("alice", uri).unwrap().as_slice(), bytes);
        assert!(files.take("alice", uri).is_err());
    }

    #[test]
    fn downloads_require_owner_bound_fresh_directional_grants() {
        let files = TransferFiles::default();
        let file = files
            .publish(
                "alice",
                "token.txt",
                "text/plain",
                Zeroizing::new(b"fixture-token".to_vec()),
            )
            .unwrap();
        let params = DownloadParams {
            _meta: BTreeMap::new(),
            uri: file.uri.clone(),
        };
        assert!(files.authorize_download("bob", &params, &base()).is_err());
        let first = files.authorize_download("alice", &params, &base()).unwrap();
        let second = files.authorize_download("alice", &params, &base()).unwrap();
        let id = file_id(&file.uri).unwrap();
        assert!(files.download(id, &token(&first, "download")).is_err());
        assert!(
            files
                .upload(id, &token(&second, "download"), vec![])
                .is_err()
        );
        assert!(files.take("alice", &file.uri).is_err());
        assert_eq!(
            files
                .download(id, &token(&second, "download"))
                .unwrap()
                .as_slice(),
            b"fixture-token"
        );
        assert!(files.download(id, &token(&second, "download")).is_err());
    }

    #[test]
    fn expired_files_and_grants_cannot_transfer_or_consume_bytes() {
        let files = TransferFiles::default();
        let upload = files
            .authorize_upload("alice", serde_json::from_value(json!({})).unwrap(), &base())
            .unwrap();
        let uri = upload["file"]["uri"].as_str().unwrap();
        let id = file_id(uri).unwrap();
        files
            .entries
            .lock()
            .unwrap()
            .get_mut(id)
            .unwrap()
            .grant
            .as_mut()
            .unwrap()
            .expires = Instant::now();
        assert!(
            files
                .upload(id, &token(&upload, "upload"), vec![1])
                .is_err()
        );
        let file = files
            .publish("alice", "token", "text/plain", Zeroizing::new(vec![1]))
            .unwrap();
        files
            .entries
            .lock()
            .unwrap()
            .get_mut(file_id(&file.uri).unwrap())
            .unwrap()
            .expires = Instant::now();
        assert!(
            files
                .authorize_download(
                    "alice",
                    &DownloadParams {
                        _meta: BTreeMap::new(),
                        uri: file.uri.clone()
                    },
                    &base()
                )
                .is_err()
        );
        assert!(files.take("alice", &file.uri).is_err());
        assert!(
            !files
                .entries
                .lock()
                .unwrap()
                .contains_key(file_id(&file.uri).unwrap())
        );
    }

    #[test]
    fn resource_bounds_apply_to_metadata_bytes_and_registry_capacity() {
        let files = TransferFiles::default();
        for params in [
            json!({"size":MAX_UPLOAD_BYTES+1}),
            json!({"name":"x".repeat(256)}),
            json!({"digest":{"algorithm":"md5","value":"ignored"}}),
        ] {
            assert!(
                files
                    .authorize_upload("alice", serde_json::from_value(params).unwrap(), &base())
                    .is_err()
            );
        }
        assert!(
            files
                .publish(
                    "alice",
                    "token",
                    "text/plain",
                    Zeroizing::new(vec![0; MAX_DOWNLOAD_BYTES + 1])
                )
                .is_err()
        );
        for _ in 0..MAX_ENTRIES {
            files
                .authorize_upload("alice", serde_json::from_value(json!({})).unwrap(), &base())
                .unwrap();
        }
        assert!(
            files
                .authorize_upload("alice", serde_json::from_value(json!({})).unwrap(), &base())
                .is_err()
        );
        assert!(file_id("mcp-file://komodo/../../other").is_err());
    }

    #[tokio::test]
    async fn shutdown_erases_pending_bytes() {
        let files = TransferFiles::default();
        files
            .publish("alice", "token", "text/plain", Zeroizing::new(vec![1]))
            .unwrap();
        let cancellation = tokio_util::sync::CancellationToken::new();
        cancellation.cancel();
        files.expire_until_shutdown(cancellation).await;
        assert!(files.entries.lock().unwrap().is_empty());
    }
}
