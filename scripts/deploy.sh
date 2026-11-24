#!/usr/bin/env bash
# Deploy the registry to any LEZ network, initialise it, publish its IDL and
# source to Logos Storage, and register it in itself as a deployer entry.
#
#     LEZ_SEQUENCER_URL=... LEZ_STORAGE_URL=... LEE_WALLET_HOME_DIR=... \
#         ./scripts/deploy.sh <funded-fee-payer-account>
#
# Needs `spel` on PATH (or SPEL_BIN), `cargo risczero`, Docker, and a clean
# git checkout (the source archive is `git archive HEAD`).
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
FEE_PAYER=${1:?usage: deploy.sh <fee-payer-account>}
: "${LEZ_SEQUENCER_URL:?set LEZ_SEQUENCER_URL}"
: "${LEZ_STORAGE_URL:?set LEZ_STORAGE_URL}"
: "${LEE_WALLET_HOME_DIR:?set LEE_WALLET_HOME_DIR}"
SPEL=${SPEL_BIN:-spel}
export SPEL_BIN=$SPEL SEQUENCER_URL=$LEZ_SEQUENCER_URL LEZ_FEE_PAYER=$FEE_PAYER

[ -z "$(git -C "$ROOT" status --porcelain)" ] || { echo "working tree is not clean" >&2; exit 1; }
COMMIT=$(git -C "$ROOT" rev-parse HEAD)

cd "$ROOT"
cargo build --release -q -p lez-registry
REG=$ROOT/target/release/lez-registry
cargo risczero build --manifest-path programs/registry/methods/guest/Cargo.toml
BIN=programs/registry/methods/guest/target/riscv32im-risc0-zkvm-elf/docker/lez_registry.bin

REGISTRY=$("$SPEL" deploy "$BIN" --fee-payer "$FEE_PAYER")
export LEZ_REGISTRY_ID=$REGISTRY
echo "registry program id: $REGISTRY"

"$REG" init --payer "$FEE_PAYER"
"$REG" register --program "$REGISTRY" --name lez-registry --version "$(cargo pkgid -p lez-registry | sed 's/.*[#@]//')" \
    --author-name "LP-0023 registry" --description "LEZ Program Registry (LP-0023)" \
    --idl artifacts/lez_registry-idl.json --source-repo "$ROOT" --commit "$COMMIT" \
    --source-url https://github.com/ZQ051031/logos-lambda-lp0023 \
    --build-config '{"builder":"risc0-docker","risc0":"3.0.5","manifest":"programs/registry/methods/guest/Cargo.toml","binary":"lez_registry"}'
"$REG" lookup "$REGISTRY"
echo
echo "Record this program id in README.md: $REGISTRY"
