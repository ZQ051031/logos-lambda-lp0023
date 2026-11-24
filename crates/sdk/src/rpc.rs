//! Minimal JSON-RPC client for the LEZ sequencer.

use crate::{Error, Result};
use lee_core::account::{Account, AccountId};
use serde_json::{json, Value};

pub struct RpcClient {
    url: String,
    agent: ureq::Agent,
}

impl RpcClient {
    #[must_use]
    pub fn new(url: &str) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(30))
            .build();
        Self {
            url: url.to_string(),
            agent,
        }
    }

    pub fn call(&self, method: &str, params: Value) -> Result<Value> {
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
        let resp: Value = self
            .agent
            .post(&self.url)
            .send_json(body)
            .map_err(|e| Error::Rpc(format!("{method}: {e}")))?
            .into_json()
            .map_err(|e| Error::Rpc(format!("{method}: {e}")))?;
        if let Some(err) = resp.get("error") {
            return Err(Error::Rpc(format!("{method}: {err}")));
        }
        resp.get("result")
            .cloned()
            .ok_or_else(|| Error::Rpc(format!("{method}: no result")))
    }

    /// Unknown accounts come back as the default (unowned, empty) account.
    pub fn get_account(&self, id: &AccountId) -> Result<Account> {
        let id = serde_json::to_value(id).map_err(|e| Error::Decode(e.to_string()))?;
        let v = self.call("getAccount", json!([id]))?;
        serde_json::from_value(v).map_err(|e| Error::Decode(format!("account: {e}")))
    }

    pub fn last_block_id(&self) -> Result<u64> {
        let v = self.call("getLastBlockId", json!([]))?;
        v.as_u64()
            .ok_or_else(|| Error::Decode(format!("block id: {v}")))
    }
}
