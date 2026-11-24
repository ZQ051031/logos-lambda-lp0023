#!/usr/bin/env bash
# End-to-end test of the LEZ Program Registry against a real LEZ sequencer in
# standalone mode and a real Logos Storage node. Runs from a clean checkout:
#
#     ./scripts/e2e-standalone.sh
#
# Needs: git, curl, unzip, python3, Rust (rustup), Docker, `cargo risczero`
# (rzup) for the reproducible guest build, and on Debian/Ubuntu
# `libpcsclite-dev libssl-dev pkg-config clang libclang-dev` for SPEL and LEZ. Everything else (SPEL, LEZ
# sequencer + wallet, Logos Storage) is fetched at pinned versions into
# $WORK_DIR. Set SPEL_BIN / WALLET_BIN / SEQUENCER_BIN / STORAGE_URL to reuse
# existing installs. Debug network only: fees are paid by the LEZ debug
# genesis account (imported via SPEL's scripts/lib/fee-payer.sh).
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=${WORK_DIR:-$ROOT/.e2e}
SPEL_REV=c6f73ce21e4805f8430f03afdbdedf0b7e479a5e
LEZ_TAG=v0.2.5-rc2
STORAGE_VERSION=v0.5.3
SEQ_PORT=3040
STORAGE_PORT=${STORAGE_PORT:-8080}
export RISC0_DEV_MODE=1
export LEZ_SEQUENCER_URL=http://127.0.0.1:$SEQ_PORT
export SEQUENCER_URL=$LEZ_SEQUENCER_URL
export LEZ_REGISTRY_HOME=$WORK/registry-home

PASS=0
log() { printf '\n\033[1;34m==> %s\033[0m\n' "$*"; }
ok() { PASS=$((PASS + 1)); printf '\033[32m  PASS\033[0m %s\n' "$*"; }
fail() { printf '\033[31m  FAIL\033[0m %s\n' "$*"; exit 1; }

mkdir -p "$WORK/src" "$WORK/logs"
PIDS=()
cleanup() {
    for p in "${PIDS[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null || true; done
    docker rm -f lez-registry-e2e-storage >/dev/null 2>&1 || true
}
trap cleanup EXIT

# ---- toolchain -------------------------------------------------------------
log "Building tools (SPEL $SPEL_REV, LEZ $LEZ_TAG)"
if [ -z "${SPEL_BIN:-}" ]; then
    [ -d "$WORK/src/spel" ] || git clone -q https://github.com/logos-co/spel.git "$WORK/src/spel"
    git -C "$WORK/src/spel" checkout -q "$SPEL_REV"
    (cd "$WORK/src/spel" && cargo build --release -q -p spel)
    SPEL_BIN=$WORK/src/spel/target/release/spel
fi
if [ -z "${SPEL_DIR:-}" ]; then
    [ -d "$WORK/src/spel" ] || git clone -q https://github.com/logos-co/spel.git "$WORK/src/spel"
    SPEL_DIR=$WORK/src/spel
fi
if [ -z "${WALLET_BIN:-}" ] || [ -z "${SEQUENCER_BIN:-}" ]; then
    [ -d "$WORK/src/lez" ] || git clone -q --depth 1 --branch "$LEZ_TAG" \
        https://github.com/logos-blockchain/logos-execution-zone.git "$WORK/src/lez"
    (cd "$WORK/src/lez" && cargo build --release -q -p wallet -p sequencer_service \
        --features sequencer_service/standalone)
    WALLET_BIN=$WORK/src/lez/target/release/wallet
    SEQUENCER_BIN=$WORK/src/lez/target/release/sequencer_service
fi
LEZ_CONFIGS=${LEZ_CONFIGS:-$WORK/src/lez/lez}
[ -d "$LEZ_CONFIGS/sequencer/service/configs/debug" ] || {
    [ -d "$WORK/src/lez" ] || git clone -q --depth 1 --branch "$LEZ_TAG" \
        https://github.com/logos-blockchain/logos-execution-zone.git "$WORK/src/lez"
    LEZ_CONFIGS=$WORK/src/lez/lez
}
(cd "$ROOT" && cargo build --release -q -p lez-registry)
REG="$ROOT/target/release/lez-registry"
SPEL="$SPEL_BIN"
IDL="$ROOT/artifacts/lez_registry-idl.json"
CLOCK=$(python3 -c "print(b'/LEZ/ClockProgramAccount/0000001'.hex())")
export SPEL_BIN

# ---- registry guest (reproducible Docker build) ----------------------------
REGISTRY_BIN=${REGISTRY_BIN:-}
if [ -z "$REGISTRY_BIN" ]; then
    log "Building the registry guest with cargo risczero (Docker)"
    (cd "$ROOT" && cargo risczero build --manifest-path programs/registry/methods/guest/Cargo.toml) \
        > "$WORK/logs/guest-build.log" 2>&1 || { tail -30 "$WORK/logs/guest-build.log"; fail "guest build"; }
    REGISTRY_BIN=$ROOT/programs/registry/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/lez_registry.bin
fi
BUILT_IMAGE_ID=$("$SPEL" -- program-id "$REGISTRY_BIN" 2>/dev/null | grep -oE '[0-9a-f]{64}' | head -1 || true)
echo "  registry ImageID: ${BUILT_IMAGE_ID:-unknown}"

# ---- Logos Storage node ----------------------------------------------------
if [ -z "${STORAGE_URL:-}" ]; then
    log "Starting Logos Storage $STORAGE_VERSION"
    SDIR=$WORK/storage
    mkdir -p "$SDIR"
    BIN=logos-storage-linux-amd64-$STORAGE_VERSION
    if [ ! -x "$SDIR/$BIN" ]; then
        curl -fsSL -o "$SDIR/storage.zip" \
            "https://github.com/logos-storage/logos-storage-nim/releases/download/$STORAGE_VERSION/$BIN.zip"
        (cd "$SDIR" && unzip -o -q storage.zip && chmod +x "$BIN")
    fi
    rm -rf "$SDIR/data" && mkdir -p "$SDIR/data" && chmod 700 "$SDIR/data"
    ARGS=(--data-dir=/s/data --no-bootstrap-node --api-port="$STORAGE_PORT" --nat=extip:127.0.0.1)
    # Release binaries need glibc >= 2.38; run them in Ubuntu 24.04 on older hosts.
    docker run -d --name lez-registry-e2e-storage --network host -u "$(id -u):$(id -g)" \
        -v "$SDIR:/s" "${UBUNTU_IMAGE:-mirror.gcr.io/library/ubuntu:24.04}" "/s/$BIN" "${ARGS[@]}" \
        > /dev/null
    STORAGE_URL=http://127.0.0.1:$STORAGE_PORT/api/storage/v1
    for _ in $(seq 60); do curl -fs "$STORAGE_URL/debug/info" > /dev/null 2>&1 && break; sleep 1; done
    curl -fs "$STORAGE_URL/debug/info" > /dev/null || { docker logs lez-registry-e2e-storage | tail; fail "storage node did not start"; }
fi
export LEZ_STORAGE_URL=$STORAGE_URL
ok "Logos Storage node up at $STORAGE_URL"

# ---- standalone sequencer --------------------------------------------------
log "Starting LEZ sequencer (standalone)"
rm -rf "$WORK/seq" && mkdir -p "$WORK/seq"
python3 - "$LEZ_CONFIGS/sequencer/service/configs/debug/sequencer_config.json" "$WORK/seq" <<'PY'
import json, sys
c = json.load(open(sys.argv[1]))
c["home"] = sys.argv[2]
c["block_create_timeout"] = "2s"
json.dump(c, open(sys.argv[2] + "/sequencer_config.json", "w"), indent=2)
PY
(cd "$WORK/seq" && RUST_LOG=info,kameo=warn "$SEQUENCER_BIN" sequencer_config.json > "$WORK/logs/sequencer.log" 2>&1) &
PIDS+=($!)
for _ in $(seq 120); do
    curl -fs -X POST -H 'Content-Type: application/json' \
        -d '{"jsonrpc":"2.0","id":1,"method":"getLastBlockId","params":[]}' "$LEZ_SEQUENCER_URL" > /dev/null 2>&1 && break
    sleep 1
done
curl -fs -X POST -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"getLastBlockId","params":[]}' "$LEZ_SEQUENCER_URL" > /dev/null \
    || { tail -20 "$WORK/logs/sequencer.log"; fail "sequencer did not start"; }
ok "sequencer up at $LEZ_SEQUENCER_URL"

# ---- wallets ---------------------------------------------------------------
# shellcheck source=/dev/null
source "$SPEL_DIR/scripts/lib/fee-payer.sh"
new_wallet() { # dir
    rm -rf "$1" && mkdir -p "$1"
    cp "$LEZ_CONFIGS/wallet/configs/debug/wallet_config.json" "$1/"
}
new_account() { # wallet-dir
    printf 'e2e\n' | LEE_WALLET_HOME_DIR=$1 "$WALLET_BIN" account new public 2>&1 \
        | sed -n 's/.*Public\/\([A-Za-z0-9]*\).*/\1/p' | tail -1
}
DEPLOYER_W=$WORK/wallet-deployer
ALICE_W=$WORK/wallet-alice
CAROL_W=$WORK/wallet-carol
for w in "$DEPLOYER_W" "$ALICE_W" "$CAROL_W"; do new_wallet "$w"; done
FEE_PAYER=$(LEE_WALLET_HOME_DIR=$DEPLOYER_W lez_fee_payer "$WALLET_BIN" e2e)
LEE_WALLET_HOME_DIR=$ALICE_W lez_fee_payer "$WALLET_BIN" e2e > /dev/null
[ -n "$FEE_PAYER" ] || fail "could not import the debug fee payer"
export LEZ_FEE_PAYER=$FEE_PAYER
ALICE=$(new_account "$ALICE_W")
BOB=$(new_account "$ALICE_W")
CAROL=$(new_account "$CAROL_W")
echo "  fee payer $FEE_PAYER, alice $ALICE, bob $BOB, carol $CAROL"

as() { # wallet-dir cmd...
    local w=$1; shift
    LEE_WALLET_HOME_DIR=$w "$@"
}

# ---- deploy ----------------------------------------------------------------
log "Deploying the registry and a demo program"
REGISTRY=$(as "$DEPLOYER_W" "$SPEL" deploy "$REGISTRY_BIN" --fee-payer "$FEE_PAYER" 2> "$WORK/logs/deploy-registry.log") \
    || { tail "$WORK/logs/deploy-registry.log"; fail "registry deploy"; }
DEMO=$(as "$DEPLOYER_W" "$SPEL" deploy "$REGISTRY_BIN" --fee-payer "$FEE_PAYER" 2> "$WORK/logs/deploy-demo.log") \
    || { tail "$WORK/logs/deploy-demo.log"; fail "demo deploy"; }
export LEZ_REGISTRY_ID=$REGISTRY
echo "REGISTRY=$REGISTRY" > "$WORK/registry.env"
ok "registry deployed at $REGISTRY (local standalone sequencer)"
ok "demo program deployed at $DEMO"

as "$DEPLOYER_W" "$REG" init --payer "$FEE_PAYER" > /dev/null || fail "init"
ok "registry state initialised"

# ---- register the registry in itself --------------------------------------
log "Registering the registry in itself (deployer entry)"
COMMIT=$(git -C "$ROOT" rev-parse HEAD)
BUILD_CONFIG='{"builder":"risc0-docker","risc0":"3.0.5","manifest":"programs/registry/methods/guest/Cargo.toml","binary":"lez_registry"}'
as "$DEPLOYER_W" "$REG" register --program "$REGISTRY" --name lez-registry --version 0.1.0 \
    --author-name "LP-0023 registry" --description "LEZ Program Registry (LP-0023)" \
    --idl "$IDL" --source-repo "$ROOT" --commit "$COMMIT" \
    --source-url https://github.com/ZQ051031/logos-lambda-lp0023 --build-config "$BUILD_CONFIG" \
    || fail "self-registration"
"$REG" --json lookup "$REGISTRY" > "$WORK/self.json"
python3 - "$WORK/self.json" <<'PY' || fail "self entry contents"
import json, sys
r = json.load(open(sys.argv[1]))
assert len(r) == 1, r
e = r[0]
assert e["kind"] == "Deployer" and e["verified_deployer"], e
assert e["idl"]["status"] == "available", e["idl"]
assert e["idl"]["idl"]["name"] == "lez_registry"
assert e["registered_at"] > 0
PY
ok "registry resolves in itself: deployer entry, IDL fetched from Logos Storage"

# ---- unauthorised deployer entry -------------------------------------------
log "Unauthorised attempts to create a deployer entry"
if as "$ALICE_W" "$REG" register --program "$DEMO" --name stolen --version 6.6.6 --author-name mallory \
    --description "not the deployer" --idl-pointer logos-storage://zX --code-pointer logos-storage://zY \
    --commit x --build-config "$BUILD_CONFIG" > "$WORK/logs/unauth1.log" 2>&1; then
    fail "deployer entry by a non-deployer was accepted"
fi
grep -q "Signing key not found" "$WORK/logs/unauth1.log" || { cat "$WORK/logs/unauth1.log"; fail "unexpected error"; }
ok "non-deployer cannot sign for the program header (wallet refuses: no header key)"
if as "$ALICE_W" "$REG" register --program "$ALICE" --name fake --version 1 --author-name mallory \
    --description "own account posing as a program" --idl-pointer logos-storage://zX \
    --code-pointer logos-storage://zY --commit x --build-config "$BUILD_CONFIG" > "$WORK/logs/unauth2.log" 2>&1; then
    fail "deployer entry for a non-program account was accepted"
fi
grep -q "rejected on-chain" "$WORK/logs/unauth2.log" || { cat "$WORK/logs/unauth2.log"; fail "unexpected error"; }
[ -z "$("$REG" --json lookup "$ALICE" | python3 -c 'import json,sys; print(json.load(sys.stdin) or "")')" ] \
    || fail "entry exists after rejection"
ok "signed transaction with a non-program account as target was rejected on-chain (no entry created)"

# ---- deployer + third-party entries for the demo program --------------------
log "Deployer and third-party entries for the demo program"
DEMO_IDL_PTR=$("$REG" publish "$IDL")
as "$DEPLOYER_W" "$REG" register --program "$DEMO" --name demo-counter --version 1.0.0 \
    --author-name "Demo Deployer" --description "Demo program registered by its deployer" \
    --idl-pointer "$DEMO_IDL_PTR" --source-repo "$ROOT" --commit "$COMMIT" --build-config "$BUILD_CONFIG" \
    || fail "demo deployer entry"
ok "deployer entry for the demo program"
as "$ALICE_W" "$REG" register --program "$DEMO" --third-party --author "$ALICE" --name demo-counter-notes \
    --version 1.0.0 --author-name "Alice Auditor" --description "Third-party description of the demo program" \
    --idl-pointer logos-storage://zDvZRwzmCEFQUd1iuBo9Ftb8w7pb6jc7q5Zjz13uaNRXfPcaAyk0 \
    --code-pointer logos-storage://zUnknownSource --commit "$COMMIT" --build-config "$BUILD_CONFIG" \
    || fail "third-party entry"
ok "third-party entry by alice"
"$REG" --json lookup "$DEMO" > "$WORK/demo.json"
python3 - "$WORK/demo.json" <<'PY' || fail "demo lookup"
import json, sys
r = json.load(open(sys.argv[1]))
kinds = [(e["kind"], e["verified_deployer"]) for e in r]
assert kinds == [("Deployer", True), ("ThirdParty", False)], kinds
assert r[0]["idl"]["status"] == "available"
assert r[1]["idl"]["status"] == "unavailable", r[1]["idl"]
assert r[1]["name"] == "demo-counter-notes"
PY
ok "lookup distinguishes deployer from third-party entry from chain state alone"
ok "unresolvable IDL shown as unavailable; metadata still returned"
"$REG" lookup "$DEMO" | grep -q "IDL unavailable" || fail "text view lacks 'IDL unavailable'"

# ---- updates ----------------------------------------------------------------
log "Updates"
as "$ALICE_W" "$REG" update --program "$DEMO" --author "$ALICE" --version 1.1.0 \
    --description "Updated by its author" || fail "author update"
ok "author updated her entry"
ALICE_ENTRY=$("$REG" pda --program "$DEMO" --author "$ALICE" | sed -n 's/^third_party_entry //p')
as "$ALICE_W" "$SPEL" --idl "$IDL" -p "$REGISTRY" --fee-payer "$FEE_PAYER" -- update-third-party-entry \
    --entry "$ALICE_ENTRY" --author "$BOB" --target-program "$DEMO" --clock "$CLOCK" --version 9.9.9 \
    --description hijack --idl-pointer logos-storage://zX --code-pointer logos-storage://zY --source-url "" \
    --commit x --build-config "$BUILD_CONFIG" > "$WORK/logs/hijack.log" 2>&1 || true
V=$("$REG" --json lookup "$DEMO" | python3 -c 'import json,sys; print(json.load(sys.stdin)[1]["version"])')
[ "$V" = "1.1.0" ] || fail "non-author update was applied (version $V)"
ok "non-author update signed by bob was rejected on-chain (version still $V)"
as "$DEPLOYER_W" "$REG" update --program "$DEMO" --version 1.0.1 --idl-pointer "$DEMO_IDL_PTR" \
    || fail "deployer update"
ok "deployer updated the deployer entry (header key signs)"

# ---- authorship transfer ----------------------------------------------------
log "Authorship transfer"
if as "$ALICE_W" "$REG" transfer --entry "$ALICE_ENTRY" --from "$ALICE" --to "$CAROL" \
    --new-author-name Carol > "$WORK/logs/transfer1.log" 2>&1; then
    fail "transfer without the new author's signature was accepted"
fi
ok "transfer without the recipient's signature refused"
as "$ALICE_W" "$REG" transfer --entry "$ALICE_ENTRY" --from "$ALICE" --to "$BOB" --new-author-name "Bob Reviewer" \
    || fail "two-party transfer"
ok "two-party transfer alice -> bob"
if as "$ALICE_W" "$REG" update --program "$DEMO" --author "$ALICE" --version 2.0.0 > /dev/null 2>&1; then
    fail "previous author could still update"
fi
ok "previous author can no longer update"

# ---- enumeration and search -------------------------------------------------
log "Enumeration and search"
N=$("$REG" --json list | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))')
[ "$N" = 3 ] || fail "list returned $N entries, expected 3"
ok "list enumerates all $N entries"
[ "$("$REG" --json search --name demo | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))')" = 2 ] || fail "search by name"
ok "search by name"
[ "$("$REG" --json search --author bob | python3 -c 'import json,sys; print([e["name"] for e in json.load(sys.stdin)])')" = "['demo-counter-notes']" ] \
    || fail "search by author"
ok "search by author (follows the transfer)"
[ "$("$REG" --json search --kind deployer | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))')" = 2 ] || fail "search by kind"
ok "filter by entry kind"

# ---- source verification ----------------------------------------------------
log "Source verification of the registry's own entry"
if [ "${E2E_SKIP_REBUILD:-0}" = 1 ]; then
    "$REG" verify "$REGISTRY" --binary "$REGISTRY_BIN" --mark || fail "verify"
else
    "$REG" verify "$REGISTRY" --mark || fail "verify (rebuild from Logos Storage source)"
fi
"$REG" lookup "$REGISTRY" | grep -q "LOCALLY VERIFIED" || fail "local verification mark missing"
ok "rebuilt source matches the deployed ImageID; marked locally verified"

log "All $PASS checks passed"
