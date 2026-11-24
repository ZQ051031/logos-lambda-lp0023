//! Client SDK for the LEZ Program Registry.
//!
//! Every read goes straight to the configured LEZ sequencer over JSON-RPC
//! (`getAccount`); there is no indexer. Entries are found either by deriving
//! their PDA from a program id (lookup) or by walking the dense enumeration
//! slots `0..entry_count` (list and search). IDLs and source archives are
//! fetched from a Logos Storage node.

pub mod pda;
pub mod rpc;
pub mod storage;
pub mod verify;
pub mod write;

use lee_core::account::AccountId;
pub use registry_core::{EntryKind, RegistryEntry, RegistryState};
use serde::Serialize;

pub use rpc::RpcClient;
pub use storage::StorageClient;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("rpc: {0}")]
    Rpc(String),
    #[error("storage: {0}")]
    Storage(String),
    #[error("decode: {0}")]
    Decode(String),
    #[error("registry not initialised at {0}")]
    NotInitialised(AccountId),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Connection settings. Only the sequencer and a Logos Storage node are used.
#[derive(Clone, Debug)]
pub struct Config {
    pub sequencer_url: String,
    pub storage_url: String,
    /// The registry program's address (its program-loader header account).
    pub registry_id: AccountId,
}

/// IDL resolution outcome. Never an error: metadata stays viewable.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum IdlStatus {
    Available { idl: serde_json::Value },
    Unavailable { reason: String },
}

impl IdlStatus {
    #[must_use]
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }
}

/// An entry together with how it was authenticated from chain state.
#[derive(Clone, Debug, Serialize)]
pub struct EntryView {
    /// The entry account's address.
    pub entry_id: String,
    pub program_id: String,
    pub author: String,
    pub image_id: String,
    /// `true` when the entry is a deployer entry *and* lives at the PDA
    /// `["deployer_entry", program_id]` of the registry, owned by the registry.
    /// Only the program's header key can create that account, so clients get
    /// this from chain state alone.
    pub verified_deployer: bool,
    #[serde(flatten)]
    pub entry: RegistryEntry,
}

/// What a resolve returns: metadata plus IDL status.
#[derive(Clone, Debug, Serialize)]
pub struct Resolved {
    #[serde(flatten)]
    pub view: EntryView,
    pub idl: IdlStatus,
}

/// Search filter: case-insensitive substrings; empty means "any".
#[derive(Clone, Debug, Default)]
pub struct Query {
    pub name: Option<String>,
    pub author: Option<String>,
    pub kind: Option<EntryKind>,
}

impl Query {
    #[must_use]
    pub fn matches(&self, v: &EntryView) -> bool {
        let contains =
            |hay: &str, needle: &str| hay.to_lowercase().contains(&needle.to_lowercase());
        self.name
            .as_deref()
            .is_none_or(|n| contains(&v.entry.name, n))
            && self
                .author
                .as_deref()
                .is_none_or(|a| contains(&v.entry.author_name, a) || v.author == a)
            && self.kind.is_none_or(|k| v.entry.kind == k)
    }
}

/// Image id words as the hex string `spel program-id` and `cargo risczero` print.
#[must_use]
pub fn image_id_hex(words: &[u32; 8]) -> String {
    words
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Read-only registry client.
pub struct Registry {
    pub config: Config,
    pub rpc: RpcClient,
    pub storage: StorageClient,
}

impl Registry {
    #[must_use]
    pub fn new(config: Config) -> Self {
        let rpc = RpcClient::new(&config.sequencer_url);
        let storage = StorageClient::new(&config.storage_url);
        Self {
            config,
            rpc,
            storage,
        }
    }

    pub fn state(&self) -> Result<RegistryState> {
        let id = pda::state(&self.config.registry_id);
        let acc = self.rpc.get_account(&id)?;
        if acc.program_owner != self.config.registry_id {
            return Err(Error::NotInitialised(self.config.registry_id));
        }
        RegistryState::try_from(&acc.data).map_err(|e| Error::Decode(e.to_string()))
    }

    /// Reads and authenticates the entry at `entry_id`. `None` if absent.
    pub fn entry_at(&self, entry_id: &AccountId) -> Result<Option<EntryView>> {
        let acc = self.rpc.get_account(entry_id)?;
        if acc.program_owner != self.config.registry_id {
            return Ok(None);
        }
        let entry = RegistryEntry::try_from(&acc.data).map_err(|e| Error::Decode(e.to_string()))?;
        let verified_deployer = entry.kind == EntryKind::Deployer
            && pda::deployer_entry(&self.config.registry_id, &entry.program_id) == *entry_id;
        Ok(Some(EntryView {
            entry_id: entry_id.to_string(),
            program_id: entry.program_id.to_string(),
            author: entry.author.to_string(),
            image_id: image_id_hex(&entry.image_id),
            verified_deployer,
            entry,
        }))
    }

    /// The deployer entry of `program_id`, found by PDA (one RPC call).
    pub fn deployer_entry(&self, program_id: &AccountId) -> Result<Option<EntryView>> {
        self.entry_at(&pda::deployer_entry(&self.config.registry_id, program_id))
    }

    /// The third-party entry of `program_id` registered by `author`, by PDA.
    pub fn third_party_entry(
        &self,
        program_id: &AccountId,
        author: &AccountId,
    ) -> Result<Option<EntryView>> {
        self.entry_at(&pda::third_party_entry(
            &self.config.registry_id,
            program_id,
            author,
        ))
    }

    /// Every entry, in registration order.
    pub fn list(&self) -> Result<Vec<EntryView>> {
        let state = self.state()?;
        let mut out = Vec::new();
        for index in 0..state.entry_count {
            let slot_id = pda::slot(&self.config.registry_id, index);
            let acc = self.rpc.get_account(&slot_id)?;
            if acc.program_owner != self.config.registry_id {
                continue;
            }
            let slot = registry_core::EntrySlot::try_from(&acc.data)
                .map_err(|e| Error::Decode(e.to_string()))?;
            if let Some(v) = self.entry_at(&slot.entry_id)? {
                out.push(v);
            }
        }
        Ok(out)
    }

    pub fn search(&self, query: &Query) -> Result<Vec<EntryView>> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|v| query.matches(v))
            .collect())
    }

    /// All entries describing `program_id`: the deployer entry (by PDA) first,
    /// then third-party entries (by enumeration).
    pub fn lookup(&self, program_id: &AccountId) -> Result<Vec<EntryView>> {
        let mut out: Vec<EntryView> = self.deployer_entry(program_id)?.into_iter().collect();
        for v in self.list()? {
            if v.entry.program_id == *program_id && v.entry.kind == EntryKind::ThirdParty {
                out.push(v);
            }
        }
        Ok(out)
    }

    /// Fetches and parses the IDL behind an entry. Failures degrade to
    /// `IdlStatus::Unavailable` and never fail the call.
    #[must_use]
    pub fn resolve_idl(&self, pointer: &str) -> IdlStatus {
        match self.storage.fetch(pointer) {
            Err(e) => IdlStatus::Unavailable {
                reason: format!("unresolvable IDL pointer: {e}"),
            },
            Ok(bytes) => match parse_idl(&bytes) {
                Ok(idl) => IdlStatus::Available { idl },
                Err(reason) => IdlStatus::Unavailable { reason },
            },
        }
    }

    /// Resolves a program id to metadata and IDL: the deployer entry if one
    /// exists, otherwise every third-party entry.
    pub fn resolve(&self, program_id: &AccountId) -> Result<Vec<Resolved>> {
        Ok(self
            .lookup(program_id)?
            .into_iter()
            .map(|view| {
                let idl = self.resolve_idl(&view.entry.idl_pointer);
                Resolved { view, idl }
            })
            .collect())
    }
}

/// Accepts a SPEL IDL: a JSON object with an `instructions` array.
pub fn parse_idl(bytes: &[u8]) -> std::result::Result<serde_json::Value, String> {
    let v: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| format!("stored IDL is not valid JSON: {e}"))?;
    if !v
        .get("instructions")
        .is_some_and(serde_json::Value::is_array)
    {
        return Err("stored IDL is not a SPEL IDL (no `instructions` array)".into());
    }
    Ok(v)
}

/// Parses a base58 or hex account id.
pub fn parse_account_id(s: &str) -> Result<AccountId> {
    let s = s.trim().trim_start_matches("Public/");
    if let Ok(id) = s.parse::<AccountId>() {
        return Ok(id);
    }
    spel_framework_core::pda::parse_bytes32(s)
        .map(AccountId::new)
        .map_err(Error::Other)
}

#[cfg(test)]
mod tests;
