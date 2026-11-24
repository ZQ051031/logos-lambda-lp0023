//! Cycle counts of the registry's register and update instructions.
//!
//! Executes the real `lez_registry` guest binary through the RISC Zero
//! executor (no proving) with the same input framing LEZ uses
//! (`lee::program::Program::write_inputs`) and prints the executor's
//! user / total cycle split per scenario as a Markdown table.

use std::borrow::Cow;

use lee::program::Program;
use lee_core::{
    account::{Account, AccountId, AccountWithMetadata, Data},
    program::ProgramHeader,
};
use registry_core::{limits, ClockData, EntryKind, Instruction, RegistryEntry, RegistryState};
use risc0_zkvm::{ExecutorEnv, ExecutorImpl, ExitCode};
use spel_framework_core::pda::{compute_pda, seed_from_str};

const PROGRAM: AccountId = AccountId::new([0x42; 32]);
const TARGET: AccountId = AccountId::new([0x50; 32]);
const AUTHOR: AccountId = AccountId::new([0x60; 32]);

fn pda(seeds: &[[u8; 32]]) -> AccountId {
    compute_pda(&PROGRAM, seeds)
}

fn account(id: AccountId, owner: AccountId, data: Vec<u8>, signed: bool) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account { program_owner: owner, data: Data::try_from(data).expect("fits"), ..Account::default() },
        is_authorized: signed,
        account_id: id,
    }
}

fn blank(id: AccountId) -> AccountWithMetadata {
    AccountWithMetadata { account: Account::default(), is_authorized: false, account_id: id }
}

fn header(signed: bool) -> AccountWithMetadata {
    let h = ProgramHeader { image_id: [7; 8], program_first_segment: AccountId::new([9; 32]), immutable: false };
    account(TARGET, registry_core::PROGRAM_LOADER_ACCOUNT_ID, h.to_bytes(), signed)
}

fn clock() -> AccountWithMetadata {
    let c = ClockData { block_id: 100, timestamp: 1_800_000_000_000 };
    account(registry_core::CLOCK_ACCOUNT_ID, AccountId::new([3; 32]), c.to_bytes(), false)
}

fn state(count: u64) -> AccountWithMetadata {
    let s = RegistryState { entry_count: count, ..RegistryState::default() };
    account(pda(&[seed_from_str(registry_core::STATE_SEED)]), PROGRAM, s.to_bytes(), false)
}

fn s(n: usize, typical: &str, max: bool) -> String {
    if max { "x".repeat(n) } else { typical.to_owned() }
}

struct Fields {
    name: String,
    version: String,
    author_name: String,
    description: String,
    idl_pointer: String,
    code_pointer: String,
    source_url: String,
    commit: String,
    build_config: String,
}

fn fields(max: bool) -> Fields {
    Fields {
        name: s(limits::NAME, "demo-counter", max),
        version: s(limits::VERSION, "1.0.0", max),
        author_name: s(limits::AUTHOR_NAME, "Alice", max),
        description: s(limits::DESCRIPTION, "A demo counter program", max),
        idl_pointer: s(limits::POINTER, "logos-storage://zDvZRwzmCEFQUd1iuBo9Ftb8w7pb6jc7q5Zjz13uaNRXfPcaAykC", max),
        code_pointer: s(limits::POINTER, "logos-storage://zDvZRwzmCEFQUd1iuBo9Ftb8w7pb6jc7q5Zjz13uaNRXfPcaAykD", max),
        source_url: s(limits::SOURCE_URL, "https://github.com/example/demo", max),
        commit: s(limits::COMMIT, "0123456789abcdef0123456789abcdef01234567", max),
        build_config: s(
            limits::BUILD_CONFIG,
            r#"{"builder":"risc0-docker","risc0":"3.0.5","manifest":"methods/guest/Cargo.toml","binary":"demo"}"#,
            max,
        ),
    }
}

fn register_ix(kind: EntryKind, f: Fields) -> Instruction {
    let slot_key = registry_core::seed_from_u64(0);
    let Fields { name, version, author_name, description, idl_pointer, code_pointer, source_url, commit, build_config } = f;
    match kind {
        EntryKind::Deployer => Instruction::RegisterDeployerEntry {
            index: 0, slot_key, name, version, author_name, description, idl_pointer, code_pointer, source_url, commit, build_config,
        },
        EntryKind::ThirdParty => Instruction::RegisterThirdPartyEntry {
            index: 0, slot_key, name, version, author_name, description, idl_pointer, code_pointer, source_url, commit, build_config,
        },
    }
}

fn update_ix(kind: EntryKind, f: Fields) -> Instruction {
    let Fields { version, description, idl_pointer, code_pointer, source_url, commit, build_config, .. } = f;
    match kind {
        EntryKind::Deployer => Instruction::UpdateDeployerEntry { version, description, idl_pointer, code_pointer, source_url, commit, build_config },
        EntryKind::ThirdParty => Instruction::UpdateThirdPartyEntry { version, description, idl_pointer, code_pointer, source_url, commit, build_config },
    }
}

fn stored_entry(kind: EntryKind, id: AccountId, f: Fields) -> AccountWithMetadata {
    let e = RegistryEntry {
        program_id: TARGET,
        kind,
        author: if kind == EntryKind::Deployer { TARGET } else { AUTHOR },
        author_name: f.author_name,
        name: f.name,
        version: f.version,
        description: f.description,
        idl_pointer: f.idl_pointer,
        code_pointer: f.code_pointer,
        source_url: f.source_url,
        commit: f.commit,
        build_config: f.build_config,
        image_id: [7; 8],
        header_immutable: false,
        registered_at: 1_700_000_000_000,
        registered_block: 1,
        updated_at: 1_700_000_000_000,
        index: 0,
    };
    account(id, PROGRAM, e.to_bytes(), false)
}

fn run(elf: &[u8], pre: &[AccountWithMetadata], ix: &Instruction) -> (u64, u64) {
    let image_id = risc0_binfmt::compute_image_id(elf).expect("image id");
    let program = Program::new_unchecked(image_id.into(), Cow::Borrowed(elf));
    let data = Program::serialize_instruction(ix).expect("instruction serialises");
    let mut builder = ExecutorEnv::builder();
    program.write_inputs(PROGRAM, None, pre, &data, &mut builder).expect("inputs");
    builder.session_limit(None);
    let env = builder.build().expect("env");
    let session = ExecutorImpl::from_elf(env, elf).expect("elf").run().expect("executes");
    assert_eq!(session.exit_code, ExitCode::Halted(0), "guest must succeed");
    (session.user_cycles, session.total_cycles)
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        "programs/registry/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/lez_registry.bin".into()
    });
    let elf = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let slot = pda(&[seed_from_str(registry_core::SLOT_SEED), registry_core::seed_from_u64(0)]);
    let dep = pda(&[seed_from_str(registry_core::DEPLOYER_ENTRY_SEED), *TARGET.value()]);
    let tp = pda(&[seed_from_str(registry_core::THIRD_PARTY_ENTRY_SEED), *TARGET.value(), *AUTHOR.value()]);
    let author = account(AUTHOR, AccountId::default(), vec![], true);

    println!("| Instruction | Field sizes | User cycles | Total cycles |");
    println!("|---|---|---:|---:|");
    for max in [false, true] {
        let size = if max { "maximum" } else { "typical" };
        let rows: Vec<(&str, Vec<AccountWithMetadata>, Instruction)> = vec![
            (
                "register_deployer_entry",
                vec![state(0), blank(dep), blank(slot), header(true), clock()],
                register_ix(EntryKind::Deployer, fields(max)),
            ),
            (
                "register_third_party_entry",
                vec![state(0), blank(tp), blank(slot), header(false), author.clone(), clock()],
                register_ix(EntryKind::ThirdParty, fields(max)),
            ),
            (
                "update_deployer_entry",
                vec![stored_entry(EntryKind::Deployer, dep, fields(max)), header(true), clock()],
                update_ix(EntryKind::Deployer, fields(max)),
            ),
            (
                "update_third_party_entry",
                vec![stored_entry(EntryKind::ThirdParty, tp, fields(max)), author.clone(), header(false), clock()],
                update_ix(EntryKind::ThirdParty, fields(max)),
            ),
        ];
        for (label, pre, ix) in rows {
            let (user, total) = run(&elf, &pre, &ix);
            println!("| `{label}` | {size} | {user} | {total} |");
        }
    }
}
