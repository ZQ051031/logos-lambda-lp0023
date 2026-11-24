//! Logos Storage REST client (`logos-storage-nim` `openapi.yaml`, base path
//! `/api/storage/v1`).
//!
//! Pointers stored on-chain look like `logos-storage://<cid>`. `file://` is
//! accepted for offline development and tests only.

use crate::{Error, Result};
use std::io::Read;

pub const SCHEME: &str = "logos-storage://";

pub struct StorageClient {
    base: String,
    agent: ureq::Agent,
}

/// Bounds a download so a hostile pointer cannot exhaust memory.
const MAX_DOWNLOAD: u64 = 64 * 1024 * 1024;

impl StorageClient {
    #[must_use]
    pub fn new(base: &str) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(60))
            .build();
        Self {
            base: base.trim_end_matches('/').to_string(),
            agent,
        }
    }

    /// Uploads bytes and returns the `logos-storage://<cid>` pointer.
    pub fn upload(&self, bytes: &[u8]) -> Result<String> {
        let cid = self
            .agent
            .post(&format!("{}/data", self.base))
            .set("Content-Type", "application/octet-stream")
            .send_bytes(bytes)
            .map_err(|e| Error::Storage(format!("upload: {e}")))?
            .into_string()
            .map_err(|e| Error::Storage(format!("upload: {e}")))?;
        let cid = cid.trim();
        if cid.is_empty() {
            return Err(Error::Storage("upload returned no CID".into()));
        }
        Ok(format!("{SCHEME}{cid}"))
    }

    /// Fetches the content behind a pointer.
    pub fn fetch(&self, pointer: &str) -> Result<Vec<u8>> {
        if let Some(cid) = pointer.strip_prefix(SCHEME) {
            if cid.is_empty() || !cid.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Err(Error::Storage(format!("malformed CID in {pointer}")));
            }
            let resp = self
                .agent
                .get(&format!("{}/data/{cid}/network/stream", self.base))
                .call()
                .map_err(|e| Error::Storage(format!("fetch {cid}: {e}")))?;
            let mut buf = Vec::new();
            resp.into_reader()
                .take(MAX_DOWNLOAD)
                .read_to_end(&mut buf)
                .map_err(|e| Error::Storage(format!("fetch {cid}: {e}")))?;
            return Ok(buf);
        }
        if let Some(path) = pointer.strip_prefix("file://") {
            return std::fs::read(path).map_err(|e| Error::Storage(format!("{path}: {e}")));
        }
        Err(Error::Storage(format!(
            "unsupported pointer scheme: {pointer}"
        )))
    }
}
