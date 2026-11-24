# LEZ Program Registry (LP-0023)

An on-chain registry of deployed [Logos Execution Zone](https://github.com/logos-blockchain/logos-execution-zone) programs, built with the [SPEL framework](https://github.com/logos-co/spel), with entry IDLs and source archives on [Logos Storage](https://github.com/logos-storage/logos-storage-nim). It ships a Rust SDK, a CLI (`lez-registry`), and a Logos Core Module built with `logos-module-builder` (`mkLogosModule`).

This repository is a solution in progress for [Logos Lambda Prize LP-0023](https://github.com/logos-co/lambda-prize/blob/master/prizes/LP-0023.md). The draft solution write-up is [`solutions/LP-0023.md`](solutions/LP-0023.md). It has **not** been submitted: see [Adoption status](#adoption-status).

## Status at a glance

| Item | Status |
|---|---|
| SPEL registry program, generated IDL (`artifacts/lez_registry-idl.json`) | Done, verified |
| Unit tests (program logic, SDK, FFI) | Passing locally and in CI |
| End-to-end flow against a real LEZ sequencer (standalone) and a real Logos Storage node | Passing locally (`scripts/e2e-standalone.sh`), wired into CI |
| Unauthorised deployer-entry attempts rejected | Demonstrated in the E2E script |
| Registry registered in itself | Done on the local standalone sequencer |
| Source verification (rebuild from Logos Storage, ImageID match, local mark) | Done on the local standalone sequencer |
| Logos Core Module (`nix build ./modules/registry`) | Builds locally, built in CI |
| Cycle counts for register and update | Measured, see [Cycle counts](#cycle-counts) |
| **Deployment on Logos testnet 0.4** | **Blocked: testnet 0.4 has no published endpoints yet** |
| Module catalog release (`logos-modules-release-base` fork) | Not done yet, see [Next steps](#blockers-and-next-steps) |
| Adoption evidence | **NONE YET** |

## Deployed program ID

**Logos testnet 0.4: not deployed.** No program ID is recorded here because none exists yet.

When this was written, the Logos documentation's testnet endpoint table ([`docs/get-started/testnet-endpoints.md` in logos-docs](https://github.com/logos-co/logos-docs)) lists every Testnet 0.4 endpoint (sequencer, indexer, explorer, faucet) as `TBD`. The public sequencer at `https://testnet.lez.logos.co` is Testnet 0.3.2, not 0.4, and this code is built against LEZ `v0.2.5-rc2` (program-loader headers, fees), the stack paired with SPEL revision `c6f73ce`. Deploying to a different testnet version and calling it 0.4 would be misleading, so it was not done.

The registry has been deployed and exercised only on a **local standalone sequencer**, where addresses are ephemeral (every run deploys to a fresh header account). Those local IDs are printed by the E2E script and are not listed here.

Exact next step once Testnet 0.4 endpoints are published:

```bash
export LEZ_SEQUENCER_URL=<testnet 0.4 sequencer URL>
export LEZ_STORAGE_URL=<your Logos Storage node>/api/storage/v1
export LEE_WALLET_HOME_DIR=<wallet configured for testnet 0.4, with a funded account>
./scripts/deploy.sh <funded-account-id>
```

`scripts/deploy.sh` builds the guest reproducibly, deploys it, initialises the registry, publishes the IDL and source archive to Logos Storage, and registers the registry in itself as a deployer entry. The printed header account is the program ID to record in this section.

## Design

### On-chain accounts

All accounts are PDAs of the registry program (SPEL's multi-seed SHA-256 PDA derivation), so any client can compute every address without an indexer.

| Account | Seeds | Contents |
|---|---|---|
| Registry state | `"registry_state"` | entry count |
| Enumeration slot `i` | `"entry_slot"`, `i` as 32-byte little-endian | address of entry `i` |
| Deployer entry | `"deployer_entry"`, program ID | `RegistryEntry` |
| Third-party entry | `"third_party_entry"`, program ID, author | `RegistryEntry` |

`RegistryEntry` (Borsh, `programs/registry/core/src/lib.rs`) holds: program ID, kind (`Deployer` or `ThirdParty`), author account, author display name, name, version, description, IDL pointer, code pointer, optional source URL, commit, build configuration, the ImageID and immutability flag copied from the program's header at registration or update time, registration timestamp and block (read from the LEZ clock account, not supplied by the caller), last update timestamp, and the enumeration index. Every string has a hard length limit (`registry_core::limits`) to bound storage and cycles.

### Instructions

| Instruction | Signers | Effect |
|---|---|---|
| `initialize` | payer | creates the state account |
| `register_deployer_entry` | **the program's header account** | creates the deployer entry PDA and the next slot |
| `register_third_party_entry` | author | creates the author's third-party entry PDA and the next slot |
| `update_deployer_entry` | program header (the entry's author) | updates version, description, pointers, commit, build config, refreshes ImageID |
| `update_third_party_entry` | current author | same, for third-party entries |
| `transfer_authorship` | current author **and** new author | moves a third-party entry to a new author |

### Deployer vs third-party entries

In LEZ `v0.2.5` a deployed program lives at its **program-loader header account**: an account owned by the program loader (`0xFE..FE`) whose key the deployer chose and holds. That header account is the program ID. The registry treats "signed by the account that deployed the program" as "signed by the header account":

- `register_deployer_entry` requires the target account to be owned by the program loader, to decode as a `ProgramHeader`, and to sign the transaction. The entry is created at the PDA `["deployer_entry", program_id]`, so at most one deployer entry per program can exist.
- Third-party entries live at `["third_party_entry", program_id, author]`, and their author must sign.
- A client decides "deployer entry" from chain state alone: the entry account must be owned by the registry, decode with `kind == Deployer`, and sit at the deployer PDA for that program ID. The SDK exposes this as `EntryView::verified_deployer`. No off-chain service is consulted.

The demo shows two unauthorised attempts failing: a wallet without the header key cannot produce the signature at all, and a signed transaction that passes an ordinary account as the "program" is rejected on-chain (`NotAProgram`) and leaves no entry behind. A non-author update signed by a different account is also rejected on-chain.

Deployer entries are non-transferable: the header key is the deployer's identity. Third-party entries can be transferred only with both signatures.

### Discovery without an indexer

- Lookup by program ID: derive the deployer PDA and fetch it, plus scan third-party entries for that program.
- Enumerate: read the entry count from the state account, then slot `0..n`, then each entry.
- Search by name, author name or author account, and kind: client-side filtering of the enumeration.

All reads use the sequencer's `getAccount` JSON-RPC method. Searching costs `2n + 1` account reads, which is fine for thousands of entries; a cache can be layered on later without changing the trust model.

### Additional design choices

**What lives on-chain.** Only metadata and pointers: names, version, description, author, two Logos Storage pointers, an optional source URL, commit, build configuration (JSON, max 512 bytes), the ImageID and immutability flag copied from the header, and timestamps. IDLs and source archives are off-chain on Logos Storage. Copying the ImageID costs 33 bytes and lets a client spot that a program was redeployed under the same header since the entry was last updated, without a second lookup. Fields are length-limited so a maximal entry stays around 1.6 KB; cycle costs are below.

**How verified source is hosted.** The CLI publishes a deterministic `git archive` of the exact commit to Logos Storage (`lez-registry publish-source`, or `register --source-repo`), and the entry's code pointer is that CID. `lez-registry verify` downloads the archive from Logos Storage, so re-verification does not depend on GitHub or any other forge staying online. A forge URL can be recorded as well (`source_url`) for human browsing, but it is informational and never used for verification. The build configuration names the RISC Zero version, guest manifest, and binary, and the build runs in RISC Zero's pinned Docker image (`cargo risczero build`), which is what makes the ImageID reproducible.

**Trusted-signer overlay.** None is shipped. Entries are labelled only by facts derivable from chain state (deployer vs third-party) and by the user's own local verification marks. A hard-coded verifier list would reintroduce a central trust root, which this prize asks us to avoid. Local marks are stored in `~/.config/lez-registry/verified.json` (override with `LEZ_REGISTRY_HOME`); this is a plain JSON file the user can inspect, edit, or delete, and `lez-registry verified` lists it.

### External services

Only the user's configured LEZ sequencer and Logos Storage node are contacted. There is no analytics or telemetry of any kind. `source_url` is never fetched by the tooling.

## Repository layout

| Path | Contents |
|---|---|
| `programs/registry/core` | shared types, instruction enum, seeds, limits |
| `programs/registry` | pure state-transition logic and unit tests |
| `programs/registry/methods/guest` | SPEL guest (`#[lez_program]`), compiled for the RISC Zero zkVM |
| `artifacts/lez_registry-idl.json` | registry IDL generated by SPEL |
| `crates/sdk` | Rust SDK (`lez-registry-sdk`) |
| `crates/cli` | `lez-registry` CLI |
| `modules/registry` | Logos Core Module (`mkLogosModule`) and `registry_ffi` C ABI |
| `benchmark` | cycle-count benchmark of the real guest |
| `scripts/e2e-standalone.sh` | clean-environment demo and E2E test |
| `scripts/deploy.sh` | deploy, initialise, and self-register on any LEZ network |
| `tools/idl-gen` | SPEL IDL generator used by CI to check the committed IDL |

## Setup

Requirements: Linux or macOS, Git, Rust (via rustup; the toolchain is pinned in `rust-toolchain.toml`), Docker, Python 3, curl, unzip, and [`rzup`](https://dev.risczero.com/api/zkvm/install) with `cargo-risczero` 3.0.5. On Debian or Ubuntu, SPEL and the LEZ wallet also need `sudo apt-get install libpcsclite-dev libssl-dev pkg-config clang libclang-dev`. Nix is needed only for the Core Module.

```bash
git clone https://github.com/ZQ051031/logos-lambda-lp0023
cd logos-lambda-lp0023
cargo build --release -p lez-registry          # CLI at target/release/lez-registry
cargo risczero build --manifest-path programs/registry/methods/guest/Cargo.toml
```

## Demo from a clean environment

```bash
./scripts/e2e-standalone.sh
```

The script fetches SPEL and LEZ at the pinned versions, builds the `spel` CLI, the LEZ wallet, and a standalone sequencer, downloads Logos Storage `v0.5.3` and starts a local node (inside an Ubuntu 24.04 container when the host glibc is older than 2.38), builds the registry guest, and then checks:

1. The registry and a demo program are deployed; the registry is initialised.
2. The registry registers itself as a deployer entry, with its IDL and source archive on Logos Storage, and resolves to the parsed IDL.
3. Unauthorised deployer entries fail: no header key, and an on-chain rejection that leaves no entry.
4. A deployer entry and a third-party entry for the demo program are told apart from chain state.
5. An unresolvable IDL pointer is shown as `IDL unavailable` while the metadata still displays.
6. The author updates their entry; a different signer's update is rejected on-chain; the deployer updates the deployer entry.
7. Transfer without the recipient's signature fails; a two-party transfer succeeds; the previous author loses update rights.
8. `list` enumerates every entry; `search` works by name, author, and kind.
9. `verify` downloads the registry's source from Logos Storage, rebuilds it with Docker, matches the deployed ImageID, and marks the program locally verified.

Set `E2E_SKIP_REBUILD=1` to verify against the already-built binary instead of rebuilding. Existing tools can be reused with `SPEL_BIN`, `WALLET_BIN`, `SEQUENCER_BIN`, and `STORAGE_URL`. The script uses the LEZ debug network's genesis fee payer, which exists only on debug and standalone sequencers.

## CLI usage

Global options (or environment variables): `--sequencer` (`LEZ_SEQUENCER_URL`, default `http://127.0.0.1:3040`), `--storage` (`LEZ_STORAGE_URL`, default `http://127.0.0.1:8080/api/storage/v1`), `--registry` (`LEZ_REGISTRY_ID`), `--json`. Write commands sign through `spel` and the LEZ wallet in `LEE_WALLET_HOME_DIR`; the CLI never handles private keys. Pass `--fee-payer` (`LEZ_FEE_PAYER`) to choose who pays fees.

```bash
# Publish an IDL and a source archive to Logos Storage
lez-registry publish target/idl/my_program-idl.json        # prints logos-storage://<cid>
lez-registry publish-source --repo . --commit "$(git rev-parse HEAD)"

# Register as the deployer (the wallet must hold the program header key)
lez-registry register --program <PROGRAM_ID> --name my-program --version 1.0.0 \
  --author-name "Alice" --description "What it does" \
  --idl target/idl/my_program-idl.json --source-repo . --commit "$(git rev-parse HEAD)" \
  --build-config '{"builder":"risc0-docker","risc0":"3.0.5","manifest":"methods/guest/Cargo.toml","binary":"my_program"}'

# Register as a third party
lez-registry register --program <PROGRAM_ID> --third-party --author <MY_ACCOUNT> ...

# Update (author only), transfer (both parties sign)
lez-registry update --program <PROGRAM_ID> --version 1.1.0 --description "New description"
lez-registry update --program <PROGRAM_ID> --author <MY_ACCOUNT> --idl-pointer logos-storage://<cid>
lez-registry transfer --entry <ENTRY_ID> --from <ME> --to <NEW_AUTHOR> --new-author-name "Bob"

# Discovery
lez-registry lookup <PROGRAM_ID>
lez-registry list
lez-registry search --name counter --author alice --kind deployer

# Source verification
lez-registry verify <PROGRAM_ID> --mark      # rebuild from the recorded source, compare ImageID
lez-registry verified                         # list your local marks
```

`lookup` prints every entry for the program with its kind (`DEPLOYER (signed by program header)` or `THIRD-PARTY`), metadata, local verification state, and the IDL or `IDL unavailable (<reason>)`.

## SDK

Add the crate by Git path:

```toml
[dependencies]
lez-registry-sdk = { git = "https://github.com/ZQ051031/logos-lambda-lp0023" }
```

### Worked example: resolve a program ID to metadata and IDL

This is [`crates/sdk/examples/resolve.rs`](crates/sdk/examples/resolve.rs), compiled in CI:

```rust
use lez_registry_sdk::{parse_account_id, Config, IdlStatus, Registry};

let registry = Registry::new(Config {
    sequencer_url: "http://127.0.0.1:3040".into(),
    storage_url: "http://127.0.0.1:8080/api/storage/v1".into(),
    registry_id: parse_account_id("<REGISTRY_PROGRAM_ID>")?,
});
let program_id = parse_account_id("<PROGRAM_ID>")?;

for r in registry.resolve(&program_id)? {
    // Decided from chain state only: kind == Deployer, at the deployer PDA, owned by the registry.
    let kind = if r.view.verified_deployer { "deployer" } else { "third-party" };
    println!("{} {} [{kind}] by {}", r.view.entry.name, r.view.entry.version, r.view.entry.author_name);
    match r.idl {
        IdlStatus::Available { idl } => println!("IDL: {}", idl["name"]),
        // Unresolvable pointer or unparsable IDL: the metadata above is still returned.
        IdlStatus::Unavailable { reason } => println!("IDL unavailable ({reason})"),
    }
}
```

```bash
LEZ_REGISTRY_ID=<REGISTRY_PROGRAM_ID> cargo run -p lez-registry-sdk --example resolve -- <PROGRAM_ID>
```

Other entry points on `Registry`: `lookup` (entries without IDL), `list`, `search(&Query { name, author, kind })`, `deployer_entry`, `third_party_entry`, `state`, and `resolve_idl(pointer)`. `lez_registry_sdk::write::Signer` registers, updates, and transfers via `spel`, and checks the resulting account state afterwards (SPEL reports "Transaction confirmed" even when the program rejects an instruction). `lez_registry_sdk::verify` rebuilds sources and manages local marks. `lez_registry_sdk::pda` derives every registry address.

## Logos Core Module

`modules/registry` is a universal Logos Core Module (`lez_registry_module`) built with `logos-module-builder`'s `mkLogosModule`. It wraps the SDK through the `registry_ffi` C ABI (`modules/registry/ffi/include/registry_ffi.h`).

```bash
nix build ./modules/registry      # result/lib/lez_registry_module_plugin.so, libregistry_ffi.so
```

Methods (all return JSON strings `{"ok": ...}` or `{"error": "..."}`):

| Method | Purpose |
|---|---|
| `configure(config_json)` / `config()` | set or show `sequencer_url`, `storage_url`, `registry_id`, `idl_path`, `fee_payer`, `spel_bin` (defaults from `LEZ_SEQUENCER_URL`, `LEZ_STORAGE_URL`, `LEZ_REGISTRY_ID`, `LEZ_REGISTRY_IDL`, `LEZ_FEE_PAYER`) |
| `resolve(program_id)` | entries for a program, each with metadata and `idl.status` `available` or `unavailable` |
| `list()` | every entry |
| `search(name, author, kind)` | filtered entries; empty strings match anything |
| `registerEntry(request_json)` | register (`author` set for a third-party entry) |
| `updateEntry(request_json)` | author-only update |

From another module, using the generated client header (`lez_registry_module_api.h`):

```cpp
LezRegistryModule registry(/* logos API handle */);
registry.configure(R"({"registry_id":"<REGISTRY_PROGRAM_ID>"})");
QString entries = registry.resolve("<PROGRAM_ID>");
QString found = registry.search("counter", "", "deployer");
```

## Cycle counts

Measured by executing the real `lez_registry.bin` guest in the RISC Zero executor with LEZ's own input framing (`lee::program::Program::write_inputs`), in the style of LEZ's `cycle_bench`. LEZ's public execution budget is 32M cycles (`DEFAULT_PUBLIC_CYCLE_BUDGET`). "Maximum" fills every string field to its limit.

| Instruction | Field sizes | User cycles | Total cycles |
|---|---|---:|---:|
| `register_deployer_entry` | typical | 59525 | 131072 |
| `register_third_party_entry` | typical | 61902 | 131072 |
| `update_deployer_entry` | typical | 49206 | 131072 |
| `update_third_party_entry` | typical | 41060 | 131072 |
| `register_deployer_entry` | maximum | 73523 | 262144 |
| `register_third_party_entry` | maximum | 75153 | 262144 |
| `update_deployer_entry` | maximum | 77234 | 262144 |
| `update_third_party_entry` | maximum | 68245 | 262144 |

Run it with:

```bash
cargo run --release --manifest-path benchmark/Cargo.toml -- \
  programs/registry/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/lez_registry.bin
```

RISC Zero's old artifact bucket now returns 403 for the recursion circuit; if the build fails downloading `recursion_zkr.zip`, fetch it from `https://artifacts.risczero.net/zkr/744b999f0a35b3c86753311c7efb2a0054be21727095cf105af6ee7d3f4d8849.zip` and set `RECURSION_SRC_PATH` to the file.

## LP-0023 criterion mapping

| Criterion | How it is met | Status |
|---|---|---|
| On-chain registry with SPEL and a published IDL; content off-chain on Logos Storage | `programs/registry/methods/guest`, `artifacts/lez_registry-idl.json`; only metadata and pointers on-chain | Met |
| Register with name, version, author, description, IDL pointer, code pointer, timestamp; no central indexer | `register_*` instructions; timestamp from the LEZ clock; PDA lookup | Met |
| Deployer vs third-party entries; unauthorised deployer entry rejected in the demo | header-signed deployer PDA, `verified_deployer`; E2E step 3 | Met |
| Only the author updates; transfer needs both signatures | `update_*`, `transfer_authorship`; unit tests and E2E steps 6 and 7 | Met |
| IDL stored on Logos Storage | `lez-registry publish`, `register --idl` | Met (local node) |
| Lookup, enumerate, search by name and author | `lookup`, `list`, `search` | Met |
| Source pointer, commit, build config; local rebuild and "locally verified" mark | `verify --mark`, `verified` | Met |
| Operates end-to-end on testnet 0.4 | standalone sequencer only | **Blocked** (no 0.4 endpoints) |
| SDK and module for registration, resolution, search | `crates/sdk`, `modules/registry` | Met |
| CLI for register, update, lookup, search, source verification | `crates/cli` | Met |
| Graceful IDL degradation | `IdlStatus::Unavailable`; unit test and E2E step 5 | Met |
| No mandatory centralised services; analytics opt-in | only sequencer and Logos Storage; no analytics | Met |
| Cycle counts for register and update | [Cycle counts](#cycle-counts) | Met |
| Deployed and tested on testnet 0.4 | | **Blocked** |
| Linux and macOS | Linux tested; macOS untested | Partly |
| Core Module built with `mkLogosModule` | `modules/registry/flake.nix`, CI job | Met |
| Registry IDL via SPEL; registry registered in itself | E2E step 2 | Met locally; testnet pending |
| E2E tests against a standalone sequencer in CI; CI green | `.github/workflows/ci.yml` | See CI badge on `main` |
| README: setup, program ID, CLI and module usage | this file | Program ID pending |
| SDK worked example | [SDK](#sdk) | Met |

## Adoption status

**Adoption evidence so far: NONE YET.**

There are no third-party integrations, no registrations on any public network, no verified-source entries from other authors, no testimonials, and no macOS users. Nothing in this repository should be read as claiming otherwise.

To claim the prize, all of the following must exist and be linkable:

1. 5 independent third-party Logos UI Modules, on public forges with genuine commit history, that use this SDK or module to resolve program metadata or IDLs.
2. 50 entries on the official Logos zone (testnet 0.4 or its successor) for genuinely deployed programs with resolvable, parseable IDLs, from at least 20 distinct author accounts independent of this team.
3. Those registrations spread over at least 2 months, with at least 10 new registrations in each month (checked from on-chain timestamps).
4. 20 entries with a source claim an evaluator can rebuild to the deployed ImageID.
5. 20 Discord testimonials and 20 Twitter/X testimonials from real accounts describing actual use.
6. macOS users or installs among the above, as well as Linux.
7. The module published to a module catalog (fork of `logos-modules-release-base`, released with `logos-modules-release-action`) with its `logos-repo.json` URL.

The solution pull request to `logos-co/lambda-prize` will be opened only once this evidence exists, as the prize allows at most 3 submissions.

## Blockers and next steps

1. **Testnet 0.4 deployment.** Blocked until Testnet 0.4 endpoints are published. Then run `scripts/deploy.sh` (see [Deployed program ID](#deployed-program-id)), record the program ID here, and run the E2E checks against it.
2. **Module catalog.** Fork `logos-modules-release-base`, add this module, release with `logos-modules-release-action`, and record the `logos-repo.json` URL.
3. **macOS.** Build and run the CLI and module on macOS; nothing is Linux-specific by design, but it has not been tested.
4. **Upstream SPEL issue.** The `spel` CLI encodes `u64` PDA seed arguments big-endian and right-aligned (`spel-cli/src/pda.rs`), while the guest macros use the little-endian `ToSeed` encoding, so PDAs seeded by a `u64` argument mismatch for any value other than 0. The registry avoids this by seeding slots with a `[u8; 32]` `slot_key` that the program checks equals `index` in little-endian. This should be reported upstream.
5. **Adoption.** See [Adoption status](#adoption-status).

## Licence

Dual-licensed under **MIT and Apache-2.0**: see [`LICENSE`](LICENSE), [`LICENSE-MIT`](LICENSE-MIT), and [`LICENSE-APACHE`](LICENSE-APACHE).
