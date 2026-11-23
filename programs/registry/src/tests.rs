use super::*;
use lee_core::account::{Account, AccountId};
use registry_core::{EntryInput, EntryKind, RegistryEntry, RegistryState};

const REGISTRY: AccountId = AccountId::new([7; 32]);

fn acc(id: u8, authorized: bool) -> AccountWithMetadata {
    AccountWithMetadata { account: Account::default(), is_authorized: authorized, account_id: AccountId::new([id; 32]) }
}

fn with_data(mut a: AccountWithMetadata, owner: AccountId, bytes: Vec<u8>) -> AccountWithMetadata {
    a.account.program_owner = owner;
    a.account.data = Data::try_from(bytes).unwrap_or_default();
    a
}

fn header(authorized: bool) -> AccountWithMetadata {
    let h = ProgramHeader { image_id: [1, 2, 3, 4, 5, 6, 7, 8], program_first_segment: AccountId::new([9; 32]), immutable: false };
    with_data(acc(50, authorized), PROGRAM_LOADER_ACCOUNT_ID, h.to_bytes())
}

fn clock(ts: u64) -> AccountWithMetadata {
    let c = ClockData { block_id: 42, timestamp: ts };
    let mut a = with_data(acc(0, false), AccountId::new([3; 32]), c.to_bytes());
    a.account_id = CLOCK_ACCOUNT_ID;
    a
}

fn state(count: u64) -> AccountWithMetadata {
    let s = RegistryState { entry_count: count, ..RegistryState::default() };
    with_data(acc(10, false), REGISTRY, s.to_bytes())
}

fn input() -> EntryInput {
    EntryInput {
        name: "demo".into(),
        version: "1.0.0".into(),
        author_name: "alice".into(),
        description: "A demo program".into(),
        idl_pointer: "logos-storage://zIdl".into(),
        code_pointer: "logos-storage://zSrc".into(),
        source_url: String::new(),
        commit: "abc123".into(),
        build_config: "risc0-docker:3.0.5".into(),
    }
}

fn accounts(program: AccountWithMetadata, author: Option<AccountWithMetadata>) -> RegisterAccounts {
    RegisterAccounts { state: state(0), entry: acc(20, false), slot: acc(30, false), program, author, clock: clock(1_700_000) }
}

fn entry_of(diffs: &[AccountStateDiff]) -> RegistryEntry {
    let d = diffs.get(1).and_then(|d| d.post_data.clone()).unwrap_or_default();
    RegistryEntry::try_from(&d).unwrap_or_else(|_| panic!("entry decodes"))
}

#[test]
fn deployer_entry_requires_header_signature() {
    let err = register(accounts(header(false), None), EntryKind::Deployer, 0, registry_core::seed_from_u64(0), input());
    assert_eq!(err.err(), Some(RegistryError::NotSigner));
    let ok = register(accounts(header(true), None), EntryKind::Deployer, 0, registry_core::seed_from_u64(0), input()).unwrap_or_default();
    let e = entry_of(&ok);
    assert_eq!(e.kind, EntryKind::Deployer);
    assert_eq!(e.author, AccountId::new([50; 32]));
    assert_eq!(e.image_id, [1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(e.registered_at, 1_700_000);
    assert_eq!(e.registered_block, 42);
}

#[test]
fn slot_key_must_match_index() {
    let r = register(accounts(header(true), None), EntryKind::Deployer, 0, registry_core::seed_from_u64(1), input());
    assert_eq!(r.err(), Some(RegistryError::BadSlotKey));
}

#[test]
fn third_party_entry_is_flagged() {
    let ok = register(accounts(header(false), Some(acc(60, true))), EntryKind::ThirdParty, 0, registry_core::seed_from_u64(0), input()).unwrap_or_default();
    let e = entry_of(&ok);
    assert_eq!(e.kind, EntryKind::ThirdParty);
    assert_eq!(e.author, AccountId::new([60; 32]));
    let unsigned = register(accounts(header(false), Some(acc(60, false))), EntryKind::ThirdParty, 0, registry_core::seed_from_u64(0), input());
    assert_eq!(unsigned.err(), Some(RegistryError::NotSigner));
}

#[test]
fn rejects_non_program_and_bad_index_and_long_fields() {
    let fake = with_data(acc(50, true), AccountId::new([1; 32]), vec![1, 2, 3]);
    assert_eq!(register(accounts(fake, None), EntryKind::Deployer, 0, registry_core::seed_from_u64(0), input()).err(), Some(RegistryError::NotAProgram));
    assert_eq!(
        register(accounts(header(true), None), EntryKind::Deployer, 5, registry_core::seed_from_u64(5), input()).err(),
        Some(RegistryError::BadIndex { expected: 0, got: 5 })
    );
    let mut long = input();
    long.description = "x".repeat(281);
    assert!(matches!(
        register(accounts(header(true), None), EntryKind::Deployer, 0, registry_core::seed_from_u64(0), long),
        Err(RegistryError::FieldTooLong { field: "description", .. })
    ));
}

fn stored_entry(kind: EntryKind, author: u8) -> AccountWithMetadata {
    let diffs = register(
        accounts(header(kind == EntryKind::Deployer), (kind == EntryKind::ThirdParty).then(|| acc(author, true))),
        kind,
        0,
        registry_core::seed_from_u64(0),
        input(),
    )
    .unwrap_or_default();
    let e = entry_of(&diffs);
    with_data(acc(20, false), REGISTRY, e.to_bytes())
}

fn upd() -> UpdateInput {
    UpdateInput {
        version: "1.1.0".into(),
        description: "new".into(),
        idl_pointer: "logos-storage://zIdl2".into(),
        code_pointer: "logos-storage://zSrc2".into(),
        source_url: String::new(),
        commit: "def456".into(),
        build_config: "risc0-docker:3.0.5".into(),
    }
}

#[test]
fn only_author_updates() {
    let entry = stored_entry(EntryKind::ThirdParty, 60);
    let other = update(REGISTRY, entry.clone(), Some(acc(61, true)), header(false), clock(2), upd());
    assert_eq!(other.err(), Some(RegistryError::NotAuthor));
    let unsigned = update(REGISTRY, entry.clone(), Some(acc(60, false)), header(false), clock(2), upd());
    assert_eq!(unsigned.err(), Some(RegistryError::NotSigner));
    let ok = update(REGISTRY, entry, Some(acc(60, true)), header(false), clock(2), upd()).unwrap_or_default();
    let d = ok.first().and_then(|d| d.post_data.clone()).unwrap_or_default();
    let e = RegistryEntry::try_from(&d).unwrap_or_else(|_| panic!("decodes"));
    assert_eq!((e.version.as_str(), e.updated_at, e.registered_at), ("1.1.0", 2, 1_700_000));

    let dep = stored_entry(EntryKind::Deployer, 0);
    assert_eq!(update(REGISTRY, dep.clone(), None, header(false), clock(2), upd()).err(), Some(RegistryError::NotSigner));
    assert!(update(REGISTRY, dep, None, header(true), clock(2), upd()).is_ok());
}

#[test]
fn update_rejects_foreign_accounts() {
    let mut entry = stored_entry(EntryKind::ThirdParty, 60);
    entry.account.program_owner = AccountId::new([99; 32]);
    assert_eq!(
        update(REGISTRY, entry, Some(acc(60, true)), header(false), clock(2), upd()).err(),
        Some(RegistryError::NotOwnedByRegistry)
    );
}

#[test]
fn transfer_needs_both_signatures() {
    let entry = stored_entry(EntryKind::ThirdParty, 60);
    let r = transfer_authorship(REGISTRY, entry.clone(), acc(60, true), acc(61, false), "bob".into());
    assert_eq!(r.err(), Some(RegistryError::NotSigner));
    let r = transfer_authorship(REGISTRY, entry.clone(), acc(60, false), acc(61, true), "bob".into());
    assert_eq!(r.err(), Some(RegistryError::NotSigner));
    let r = transfer_authorship(REGISTRY, entry, acc(60, true), acc(61, true), "bob".into()).unwrap_or_default();
    let d = r.first().and_then(|d| d.post_data.clone()).unwrap_or_default();
    let e = RegistryEntry::try_from(&d).unwrap_or_else(|_| panic!("decodes"));
    assert_eq!(e.author, AccountId::new([61; 32]));
    let dep = stored_entry(EntryKind::Deployer, 0);
    assert_eq!(
        transfer_authorship(REGISTRY, dep, header(true), acc(61, true), "bob".into()).err(),
        Some(RegistryError::DeployerEntryNotTransferable)
    );
}
