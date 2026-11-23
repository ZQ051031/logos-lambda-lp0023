//! State transitions of the LEZ Program Registry.
//!
//! Pure functions over `AccountWithMetadata` so they are unit-testable without
//! the zkVM. The guest (`methods/guest`) wires them to SPEL instructions; SPEL
//! enforces the `signer`, `init` and PDA constraints before these run.

use borsh::BorshDeserialize;
use lee_core::{
    account::{AccountWithMetadata, BalanceDiff, Data},
    program::{AccountStateDiff, ProgramHeader},
};
use registry_core::{
    limits, ClockData, EntryInput, EntryKind, EntrySlot, RegistryEntry, RegistryState,
    CLOCK_ACCOUNT_ID, PROGRAM_LOADER_ACCOUNT_ID,
};

#[cfg(test)]
mod tests;

/// Registry errors. `code()` is the on-chain `SpelError::Custom` code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    NotAProgram,
    BadClock,
    BadIndex { expected: u64, got: u64 },
    FieldTooLong { field: &'static str, max: usize },
    EmptyField(&'static str),
    NotAuthor,
    NotSigner,
    WrongProgram,
    DeployerEntryNotTransferable,
    SameAuthor,
    Corrupt(&'static str),
    NotOwnedByRegistry,
    Overflow,
    BadSlotKey,
}

impl RegistryError {
    #[must_use]
    pub const fn code(&self) -> u32 {
        match self {
            Self::NotAProgram => 6001,
            Self::BadClock => 6002,
            Self::BadIndex { .. } => 6003,
            Self::FieldTooLong { .. } => 6004,
            Self::EmptyField(_) => 6005,
            Self::NotAuthor => 6006,
            Self::NotSigner => 6007,
            Self::WrongProgram => 6008,
            Self::DeployerEntryNotTransferable => 6009,
            Self::SameAuthor => 6010,
            Self::Corrupt(_) => 6011,
            Self::NotOwnedByRegistry => 6012,
            Self::Overflow => 6013,
            Self::BadSlotKey => 6014,
        }
    }
}

impl core::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotAProgram => write!(f, "program account is not a program-loader header"),
            Self::BadClock => write!(f, "clock account missing or malformed"),
            Self::BadIndex { expected, got } => write!(f, "slot index must be {expected}, got {got}"),
            Self::FieldTooLong { field, max } => write!(f, "{field} exceeds {max} bytes"),
            Self::EmptyField(field) => write!(f, "{field} must not be empty"),
            Self::NotAuthor => write!(f, "only the entry's author may do this"),
            Self::NotSigner => write!(f, "required signature missing"),
            Self::WrongProgram => write!(f, "program account does not match the entry"),
            Self::DeployerEntryNotTransferable => {
                write!(f, "deployer entries are bound to the deployer key and cannot be transferred")
            }
            Self::SameAuthor => write!(f, "new author equals current author"),
            Self::Corrupt(what) => write!(f, "{what} account data is corrupt"),
            Self::NotOwnedByRegistry => write!(f, "account is not owned by the registry"),
            Self::Overflow => write!(f, "counter overflow"),
            Self::BadSlotKey => write!(f, "slot_key must be the little-endian 32-byte encoding of index"),
        }
    }
}

pub type Result<T> = core::result::Result<T, RegistryError>;

fn write(pre: AccountWithMetadata, bytes: Vec<u8>) -> Result<AccountStateDiff> {
    let data = Data::try_from(bytes).map_err(|_| RegistryError::Overflow)?;
    Ok(AccountStateDiff::new(pre, BalanceDiff::Add(0), data))
}

fn check_len(field: &'static str, value: &str, max: usize, required: bool) -> Result<()> {
    if required && value.trim().is_empty() {
        return Err(RegistryError::EmptyField(field));
    }
    if value.len() > max {
        return Err(RegistryError::FieldTooLong { field, max });
    }
    Ok(())
}

/// Validates registrant-supplied fields against `registry_core::limits`.
pub fn validate_input(input: &EntryInput) -> Result<()> {
    check_len("name", &input.name, limits::NAME, true)?;
    check_len("author_name", &input.author_name, limits::AUTHOR_NAME, true)?;
    validate_mutable(
        &input.version,
        &input.description,
        &input.idl_pointer,
        &input.code_pointer,
        &input.source_url,
        &input.commit,
        &input.build_config,
    )
}

#[allow(clippy::too_many_arguments, reason = "mirrors the flat instruction arguments")]
fn validate_mutable(
    version: &str,
    description: &str,
    idl_pointer: &str,
    code_pointer: &str,
    source_url: &str,
    commit: &str,
    build_config: &str,
) -> Result<()> {
    check_len("version", version, limits::VERSION, true)?;
    check_len("description", description, limits::DESCRIPTION, true)?;
    check_len("idl_pointer", idl_pointer, limits::POINTER, true)?;
    check_len("code_pointer", code_pointer, limits::POINTER, true)?;
    check_len("source_url", source_url, limits::SOURCE_URL, false)?;
    check_len("commit", commit, limits::COMMIT, true)?;
    check_len("build_config", build_config, limits::BUILD_CONFIG, true)
}

/// Reads the program header so the entry records the deployed ImageID from
/// chain state rather than trusting the registrant.
pub fn read_header(program: &AccountWithMetadata) -> Result<ProgramHeader> {
    if program.account.program_owner != PROGRAM_LOADER_ACCOUNT_ID {
        return Err(RegistryError::NotAProgram);
    }
    ProgramHeader::from_bytes(program.account.data.as_ref()).ok_or(RegistryError::NotAProgram)
}

pub fn read_clock(clock: &AccountWithMetadata) -> Result<ClockData> {
    if clock.account_id != CLOCK_ACCOUNT_ID {
        return Err(RegistryError::BadClock);
    }
    ClockData::try_from_slice(clock.account.data.as_ref()).map_err(|_| RegistryError::BadClock)
}

fn read_state(state: &AccountWithMetadata) -> Result<RegistryState> {
    RegistryState::try_from_slice(state.account.data.as_ref())
        .map_err(|_| RegistryError::Corrupt("registry state"))
}

pub fn read_entry(entry: &AccountWithMetadata) -> Result<RegistryEntry> {
    RegistryEntry::try_from_slice(entry.account.data.as_ref())
        .map_err(|_| RegistryError::Corrupt("entry"))
}

/// Creates the singleton registry state account.
pub fn initialize(state: AccountWithMetadata, payer: AccountWithMetadata) -> Result<Vec<AccountStateDiff>> {
    Ok(vec![
        write(state, RegistryState::default().to_bytes())?,
        AccountStateDiff::unchanged(payer),
    ])
}

/// Accounts of a registration.
pub struct RegisterAccounts {
    pub state: AccountWithMetadata,
    pub entry: AccountWithMetadata,
    pub slot: AccountWithMetadata,
    pub program: AccountWithMetadata,
    /// For deployer entries this is the program header itself.
    pub author: Option<AccountWithMetadata>,
    pub clock: AccountWithMetadata,
}

/// Creates an entry plus its enumeration slot and bumps the counters.
///
/// `kind == Deployer` requires the program header account itself to have
/// signed: the deployer is whoever holds the header key, the same authority
/// the program loader requires for `UpdateHeader`.
pub fn register(
    accounts: RegisterAccounts,
    kind: EntryKind,
    index: u64,
    slot_key: [u8; 32],
    input: EntryInput,
) -> Result<Vec<AccountStateDiff>> {
    if slot_key != registry_core::seed_from_u64(index) {
        return Err(RegistryError::BadSlotKey);
    }
    let RegisterAccounts { state, entry, slot, program, author, clock } = accounts;
    validate_input(&input)?;
    let header = read_header(&program)?;
    let now = read_clock(&clock)?;
    let mut counters = read_state(&state)?;
    if index != counters.entry_count {
        return Err(RegistryError::BadIndex { expected: counters.entry_count, got: index });
    }
    let author_id = match (kind, &author) {
        (EntryKind::Deployer, _) => {
            if !program.is_authorized {
                return Err(RegistryError::NotSigner);
            }
            program.account_id
        }
        (EntryKind::ThirdParty, Some(a)) if a.is_authorized => a.account_id,
        (EntryKind::ThirdParty, _) => return Err(RegistryError::NotSigner),
    };
    counters.entry_count = counters.entry_count.checked_add(1).ok_or(RegistryError::Overflow)?;
    match kind {
        EntryKind::Deployer => {
            counters.deployer_entries =
                counters.deployer_entries.checked_add(1).ok_or(RegistryError::Overflow)?;
        }
        EntryKind::ThirdParty => {
            counters.third_party_entries =
                counters.third_party_entries.checked_add(1).ok_or(RegistryError::Overflow)?;
        }
    }
    let record = RegistryEntry {
        program_id: program.account_id,
        kind,
        author: author_id,
        author_name: input.author_name,
        name: input.name,
        version: input.version,
        description: input.description,
        idl_pointer: input.idl_pointer,
        code_pointer: input.code_pointer,
        source_url: input.source_url,
        commit: input.commit,
        build_config: input.build_config,
        image_id: header.image_id,
        header_immutable: header.immutable,
        registered_at: now.timestamp,
        registered_block: now.block_id,
        updated_at: now.timestamp,
        index,
    };
    let slot_record = EntrySlot { index, entry_id: entry.account_id };
    let mut diffs = vec![
        write(state, counters.to_bytes())?,
        write(entry, record.to_bytes())?,
        write(slot, slot_record.to_bytes())?,
        AccountStateDiff::unchanged(program),
    ];
    if let Some(a) = author {
        diffs.push(AccountStateDiff::unchanged(a));
    }
    diffs.push(AccountStateDiff::unchanged(clock));
    Ok(diffs)
}

/// Mutable fields of an update.
#[derive(Clone, Debug, Default)]
pub struct UpdateInput {
    pub version: String,
    pub description: String,
    pub idl_pointer: String,
    pub code_pointer: String,
    pub source_url: String,
    pub commit: String,
    pub build_config: String,
}

/// Author-only update. Refreshes the recorded ImageID from the header, so an
/// upgraded program's entry always reflects what is deployed now.
pub fn update(
    registry_id: lee_core::account::AccountId,
    entry: AccountWithMetadata,
    author: Option<AccountWithMetadata>,
    program: AccountWithMetadata,
    clock: AccountWithMetadata,
    input: UpdateInput,
) -> Result<Vec<AccountStateDiff>> {
    if entry.account.program_owner != registry_id {
        return Err(RegistryError::NotOwnedByRegistry);
    }
    let mut record = read_entry(&entry)?;
    if record.program_id != program.account_id {
        return Err(RegistryError::WrongProgram);
    }
    // Deployer entries are authored by the program header itself (`author == None`).
    let signer = author.as_ref().unwrap_or(&program);
    if !signer.is_authorized {
        return Err(RegistryError::NotSigner);
    }
    if record.author != signer.account_id {
        return Err(RegistryError::NotAuthor);
    }
    validate_mutable(
        &input.version,
        &input.description,
        &input.idl_pointer,
        &input.code_pointer,
        &input.source_url,
        &input.commit,
        &input.build_config,
    )?;
    let header = read_header(&program)?;
    let now = read_clock(&clock)?;
    record.version = input.version;
    record.description = input.description;
    record.idl_pointer = input.idl_pointer;
    record.code_pointer = input.code_pointer;
    record.source_url = input.source_url;
    record.commit = input.commit;
    record.build_config = input.build_config;
    record.image_id = header.image_id;
    record.header_immutable = header.immutable;
    record.updated_at = now.timestamp;
    let mut diffs = vec![write(entry, record.to_bytes())?];
    if let Some(a) = author {
        diffs.push(AccountStateDiff::unchanged(a));
    }
    diffs.push(AccountStateDiff::unchanged(program));
    diffs.push(AccountStateDiff::unchanged(clock));
    Ok(diffs)
}

/// Two-party authorship transfer of a third-party entry: both signatures required.
pub fn transfer_authorship(
    registry_id: lee_core::account::AccountId,
    entry: AccountWithMetadata,
    current: AccountWithMetadata,
    new_author: AccountWithMetadata,
    new_author_name: String,
) -> Result<Vec<AccountStateDiff>> {
    if entry.account.program_owner != registry_id {
        return Err(RegistryError::NotOwnedByRegistry);
    }
    let mut record = read_entry(&entry)?;
    if record.kind == EntryKind::Deployer {
        return Err(RegistryError::DeployerEntryNotTransferable);
    }
    if !current.is_authorized || !new_author.is_authorized {
        return Err(RegistryError::NotSigner);
    }
    if record.author != current.account_id {
        return Err(RegistryError::NotAuthor);
    }
    if new_author.account_id == current.account_id {
        return Err(RegistryError::SameAuthor);
    }
    check_len("author_name", &new_author_name, limits::AUTHOR_NAME, true)?;
    record.author = new_author.account_id;
    record.author_name = new_author_name;
    Ok(vec![
        write(entry, record.to_bytes())?,
        AccountStateDiff::unchanged(current),
        AccountStateDiff::unchanged(new_author),
    ])
}
