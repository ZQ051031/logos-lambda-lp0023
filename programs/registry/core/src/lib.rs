//! Shared types for the LEZ Program Registry.
//!
//! On-chain state is metadata and pointers only. Source archives and IDL
//! documents live on Logos Storage; entries hold their content identifiers.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::{AccountId, Data};
use serde::{Deserialize, Serialize};
use spel_framework_macros::account_type;

/// PDA seed of the singleton registry state account.
pub const STATE_SEED: &str = "registry_state";
/// PDA seed prefix of an enumeration slot, combined with the slot index.
pub const SLOT_SEED: &str = "entry_slot";
/// PDA seed prefix of a deployer entry, combined with the program id.
pub const DEPLOYER_ENTRY_SEED: &str = "deployer_entry";
/// PDA seed prefix of a third-party entry, combined with program id and author.
pub const THIRD_PARTY_ENTRY_SEED: &str = "third_party_entry";

/// Account that owns every program header (`lee_core::program::PROGRAM_LOADER_ACCOUNT_ID`).
pub const PROGRAM_LOADER_ACCOUNT_ID: AccountId = AccountId::new([0xFE; 32]);
/// The sequencer-maintained clock account updated every block.
pub const CLOCK_ACCOUNT_ID: AccountId = AccountId::new(*b"/LEZ/ClockProgramAccount/0000001");

/// Field length limits. Every byte stored on-chain costs gas, so free-form text
/// is bounded and anything large (source, IDL) is referenced by pointer.
pub mod limits {
    pub const NAME: usize = 64;
    pub const VERSION: usize = 32;
    pub const AUTHOR_NAME: usize = 64;
    pub const DESCRIPTION: usize = 280;
    pub const POINTER: usize = 128;
    pub const SOURCE_URL: usize = 200;
    pub const COMMIT: usize = 64;
    pub const BUILD_CONFIG: usize = 512;
}

/// Who created an entry. Decided on-chain by the instruction that created it:
/// `Deployer` entries can only be created with the program header's signature.
#[account_type]
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub enum EntryKind {
    Deployer,
    ThirdParty,
}

/// Singleton registry counters.
#[account_type]
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct RegistryState {
    /// Number of enumeration slots written so far; slots are `0..entry_count`.
    pub entry_count: u64,
    pub deployer_entries: u64,
    pub third_party_entries: u64,
}

/// Enumeration slot: maps a dense index to an entry account.
#[account_type]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct EntrySlot {
    pub index: u64,
    pub entry_id: AccountId,
}

/// A registry entry describing one deployed program.
#[account_type]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct RegistryEntry {
    /// The program's address: its program-loader header account.
    pub program_id: AccountId,
    pub kind: EntryKind,
    /// The account that signed the registration; the only account allowed to update it.
    pub author: AccountId,
    /// Human-readable author label (free text, not an identity proof).
    pub author_name: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// Logos Storage pointer to the program's IDL JSON, e.g. `logos-storage://<cid>`.
    pub idl_pointer: String,
    /// Logos Storage pointer to the source archive used for verification.
    pub code_pointer: String,
    /// Optional mirror (for example a git URL). Not required for verification.
    pub source_url: String,
    pub commit: String,
    /// Reproducible build configuration (see `docs/build-config.md`).
    pub build_config: String,
    /// ImageID read from the program header at registration or last update.
    /// Recorded on-chain from chain state, never supplied by the caller.
    pub image_id: [u32; 8],
    /// Whether the program header was immutable at that time.
    pub header_immutable: bool,
    /// Block timestamp (ms) and id from the clock account at registration.
    pub registered_at: u64,
    pub registered_block: u64,
    pub updated_at: u64,
    /// Enumeration slot index.
    pub index: u64,
}

/// Entry fields supplied by the registrant.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct EntryInput {
    pub name: String,
    pub version: String,
    pub author_name: String,
    pub description: String,
    pub idl_pointer: String,
    pub code_pointer: String,
    pub source_url: String,
    pub commit: String,
    pub build_config: String,
}

/// Instruction enum shared with the guest (`#[lez_program(instruction = ...)]`).
#[derive(Clone, Debug, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub enum Instruction {
    Initialize,
    RegisterDeployerEntry {
        index: u64,
        slot_key: [u8; 32],
        name: String,
        version: String,
        author_name: String,
        description: String,
        idl_pointer: String,
        code_pointer: String,
        source_url: String,
        commit: String,
        build_config: String,
    },
    RegisterThirdPartyEntry {
        index: u64,
        slot_key: [u8; 32],
        name: String,
        version: String,
        author_name: String,
        description: String,
        idl_pointer: String,
        code_pointer: String,
        source_url: String,
        commit: String,
        build_config: String,
    },
    UpdateDeployerEntry {
        version: String,
        description: String,
        idl_pointer: String,
        code_pointer: String,
        source_url: String,
        commit: String,
        build_config: String,
    },
    UpdateThirdPartyEntry {
        version: String,
        description: String,
        idl_pointer: String,
        code_pointer: String,
        source_url: String,
        commit: String,
        build_config: String,
    },
    TransferAuthorship {
        new_author_name: String,
    },
}

/// Clock account layout (mirrors `clock_core::ClockAccountData`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ClockData {
    pub block_id: u64,
    pub timestamp: u64,
}

macro_rules! data_codec {
    ($($ty:ty),*) => {$(
        impl TryFrom<&Data> for $ty {
            type Error = std::io::Error;
            fn try_from(data: &Data) -> Result<Self, Self::Error> {
                <$ty>::try_from_slice(data.as_ref())
            }
        }
        impl $ty {
            /// Borsh encoding stored in account data.
            #[must_use]
            pub fn to_bytes(&self) -> Vec<u8> {
                borsh::to_vec(self).unwrap_or_default()
            }
        }
    )*};
}
data_codec!(RegistryState, EntrySlot, RegistryEntry, ClockData);

/// Zero-padded 32-byte seed for a string literal, as SPEL's `literal("...")`.
#[must_use]
pub fn seed_from_str(s: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (dst, src) in out.iter_mut().zip(s.as_bytes()) {
        *dst = *src;
    }
    out
}

/// SPEL `arg("index")` seed for a `u64`.
#[must_use]
pub fn seed_from_u64(v: u64) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (dst, src) in out.iter_mut().zip(v.to_le_bytes()) {
        *dst = src;
    }
    out
}
