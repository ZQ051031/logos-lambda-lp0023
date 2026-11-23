//! PDA derivation, identical to the `pda = ...` constraints in the guest and
//! to what `spel pda` computes from the published IDL.

use lee_core::account::AccountId;
use registry_core::{seed_from_u64, DEPLOYER_ENTRY_SEED, SLOT_SEED, STATE_SEED, THIRD_PARTY_ENTRY_SEED};
use spel_framework_core::pda::{compute_pda, seed_from_str};

#[must_use]
pub fn state(registry: &AccountId) -> AccountId {
    compute_pda(registry, &[&seed_from_str(STATE_SEED)])
}

#[must_use]
pub fn slot(registry: &AccountId, index: u64) -> AccountId {
    compute_pda(registry, &[&seed_from_str(SLOT_SEED), &seed_from_u64(index)])
}

#[must_use]
pub fn deployer_entry(registry: &AccountId, program: &AccountId) -> AccountId {
    compute_pda(registry, &[&seed_from_str(DEPLOYER_ENTRY_SEED), program.value()])
}

#[must_use]
pub fn third_party_entry(registry: &AccountId, program: &AccountId, author: &AccountId) -> AccountId {
    compute_pda(registry, &[&seed_from_str(THIRD_PARTY_ENTRY_SEED), program.value(), author.value()])
}
