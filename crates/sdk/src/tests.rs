use super::*;
use registry_core::RegistryEntry;

fn view(name: &str, author_name: &str, kind: EntryKind) -> EntryView {
    let entry = RegistryEntry {
        program_id: AccountId::new([1; 32]),
        kind,
        author: AccountId::new([2; 32]),
        author_name: author_name.into(),
        name: name.into(),
        version: "1".into(),
        description: "d".into(),
        idl_pointer: String::new(),
        code_pointer: String::new(),
        source_url: String::new(),
        commit: String::new(),
        build_config: String::new(),
        image_id: [0; 8],
        header_immutable: false,
        registered_at: 0,
        registered_block: 0,
        updated_at: 0,
        index: 0,
    };
    EntryView {
        entry_id: String::new(),
        program_id: entry.program_id.to_string(),
        author: entry.author.to_string(),
        image_id: String::new(),
        verified_deployer: false,
        entry,
    }
}

#[test]
fn query_matching() {
    let v = view("Token Program", "Alice Labs", EntryKind::Deployer);
    assert!(Query {
        name: Some("token".into()),
        ..Query::default()
    }
    .matches(&v));
    assert!(Query {
        author: Some("alice".into()),
        ..Query::default()
    }
    .matches(&v));
    assert!(Query {
        author: Some(v.author.clone()),
        ..Query::default()
    }
    .matches(&v));
    assert!(!Query {
        name: Some("amm".into()),
        ..Query::default()
    }
    .matches(&v));
    assert!(!Query {
        kind: Some(EntryKind::ThirdParty),
        ..Query::default()
    }
    .matches(&v));
}

fn registry() -> Registry {
    Registry::new(Config {
        sequencer_url: "http://127.0.0.1:1".into(),
        storage_url: "http://127.0.0.1:1/api/storage/v1".into(),
        registry_id: AccountId::new([7; 32]),
    })
}

#[test]
fn idl_degrades_gracefully() {
    let dir = std::env::temp_dir().join(format!("lez-reg-sdk-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_default();
    let good = dir.join("good.json");
    let bad = dir.join("bad.json");
    let wrong = dir.join("wrong.json");
    std::fs::write(&good, br#"{"name":"x","instructions":[]}"#).unwrap_or_default();
    std::fs::write(&bad, b"{not json").unwrap_or_default();
    std::fs::write(&wrong, br#"{"name":"x"}"#).unwrap_or_default();
    let r = registry();
    assert!(r
        .resolve_idl(&format!("file://{}", good.display()))
        .is_available());
    for p in [
        format!("file://{}", bad.display()),
        format!("file://{}", wrong.display()),
        format!("file://{}/missing.json", dir.display()),
        "logos-storage://zUnreachable".into(),
        "logos-storage://bad/../cid".into(),
        "ipfs://elsewhere".into(),
    ] {
        assert!(
            matches!(r.resolve_idl(&p), IdlStatus::Unavailable { .. }),
            "{p}"
        );
    }
}

#[test]
fn pdas_are_distinct_and_deterministic() {
    let reg = AccountId::new([7; 32]);
    let prog = AccountId::new([8; 32]);
    let author = AccountId::new([9; 32]);
    assert_eq!(pda::slot(&reg, 3), pda::slot(&reg, 3));
    assert_ne!(pda::slot(&reg, 3), pda::slot(&reg, 4));
    assert_ne!(
        pda::deployer_entry(&reg, &prog),
        pda::third_party_entry(&reg, &prog, &author)
    );
    assert_ne!(pda::state(&reg), pda::deployer_entry(&reg, &prog));
}

#[test]
fn account_id_parsing() {
    let id = AccountId::new([5; 32]);
    assert_eq!(parse_account_id(&id.to_string()).ok(), Some(id));
    assert_eq!(parse_account_id(&format!("Public/{id}")).ok(), Some(id));
    assert_eq!(parse_account_id(&"05".repeat(32)).ok(), Some(id));
    assert!(parse_account_id("nope").is_err());
}

#[test]
fn build_config_roundtrip() {
    let s = r#"{"builder":"risc0-docker","risc0":"3.0.5","manifest":"programs/registry/methods/guest/Cargo.toml","binary":"lez_registry"}"#;
    let c = verify::BuildConfig::parse(s).unwrap_or_else(|e| panic!("{e}"));
    assert!(c.output_path(std::path::Path::new("/src")).ends_with(
        "programs/registry/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/lez_registry.bin"
    ));
}
