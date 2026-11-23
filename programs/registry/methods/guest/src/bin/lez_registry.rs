//! LEZ Program Registry guest, built with SPEL.
#![cfg_attr(not(test), no_main)]
#![allow(
    clippy::too_many_arguments,
    reason = "SPEL instruction arguments are flat for CLI friendliness"
)]

use nssa_core::account::AccountWithMetadata;
use registry_core::{EntryInput, EntryKind};
use registry_program::{RegisterAccounts, RegistryError, UpdateInput};
use spel_framework::context::ProgramContext;
use spel_framework::prelude::*;

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

fn spel_err(e: RegistryError) -> SpelError {
    SpelError::custom(e.code(), e.to_string())
}

#[lez_program(instruction = "registry_core::Instruction")]
mod lez_registry {
    #[allow(unused_imports, reason = "SPEL handlers use parent-scope types")]
    use super::*;

    /// Create the singleton registry state account. Callable once.
    #[instruction]
    pub fn initialize(
        #[account(init, pda = literal("registry_state"))] state: AccountWithMetadata,
        #[account(signer)] payer: AccountWithMetadata,
    ) -> SpelResult {
        let diffs = registry_program::initialize(state, payer).map_err(spel_err)?;
        Ok(SpelOutput::execute(diffs, vec![]))
    }

    /// Register the deployer's entry. `program` is the program header account
    /// and must sign: only the deployer holds that key.
    #[instruction]
    pub fn register_deployer_entry(
        #[account(mut, pda = literal("registry_state"))] state: AccountWithMetadata,
        #[account(init, pda = [literal("deployer_entry"), account("target_program")])] entry: AccountWithMetadata,
        #[account(init, pda = [literal("entry_slot"), arg("slot_key")])] slot: AccountWithMetadata,
        #[account(signer)] target_program: AccountWithMetadata,
        clock: AccountWithMetadata,
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
    ) -> SpelResult {
        let input = EntryInput {
            name, version, author_name, description, idl_pointer, code_pointer, source_url, commit, build_config,
        };
        let accounts = RegisterAccounts { state, entry, slot, program: target_program, author: None, clock };
        let diffs = registry_program::register(accounts, EntryKind::Deployer, index, slot_key, input).map_err(spel_err)?;
        Ok(SpelOutput::execute(diffs, vec![]))
    }

    /// Register a third-party entry for any deployed program. The entry is
    /// flagged `ThirdParty` on-chain and lives at a different PDA.
    #[instruction]
    pub fn register_third_party_entry(
        #[account(mut, pda = literal("registry_state"))] state: AccountWithMetadata,
        #[account(init, pda = [literal("third_party_entry"), account("target_program"), account("author")])] entry: AccountWithMetadata,
        #[account(init, pda = [literal("entry_slot"), arg("slot_key")])] slot: AccountWithMetadata,
        target_program: AccountWithMetadata,
        #[account(signer)] author: AccountWithMetadata,
        clock: AccountWithMetadata,
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
    ) -> SpelResult {
        let input = EntryInput {
            name, version, author_name, description, idl_pointer, code_pointer, source_url, commit, build_config,
        };
        let accounts = RegisterAccounts { state, entry, slot, program: target_program, author: Some(author), clock };
        let diffs = registry_program::register(accounts, EntryKind::ThirdParty, index, slot_key, input).map_err(spel_err)?;
        Ok(SpelOutput::execute(diffs, vec![]))
    }

    /// Update a deployer entry; the program header (its author) must sign.
    #[instruction]
    pub fn update_deployer_entry(
        ctx: ProgramContext,
        #[account(mut, pda = [literal("deployer_entry"), account("target_program")])] entry: AccountWithMetadata,
        #[account(signer)] target_program: AccountWithMetadata,
        clock: AccountWithMetadata,
        version: String,
        description: String,
        idl_pointer: String,
        code_pointer: String,
        source_url: String,
        commit: String,
        build_config: String,
    ) -> SpelResult {
        let input = UpdateInput { version, description, idl_pointer, code_pointer, source_url, commit, build_config };
        let diffs = registry_program::update(ctx.self_account_id, entry, None, target_program, clock, input).map_err(spel_err)?;
        Ok(SpelOutput::execute(diffs, vec![]))
    }

    /// Update a third-party entry; only its current author may sign.
    #[instruction]
    pub fn update_third_party_entry(
        ctx: ProgramContext,
        #[account(mut)] entry: AccountWithMetadata,
        #[account(signer)] author: AccountWithMetadata,
        target_program: AccountWithMetadata,
        clock: AccountWithMetadata,
        version: String,
        description: String,
        idl_pointer: String,
        code_pointer: String,
        source_url: String,
        commit: String,
        build_config: String,
    ) -> SpelResult {
        let input = UpdateInput { version, description, idl_pointer, code_pointer, source_url, commit, build_config };
        let diffs = registry_program::update(ctx.self_account_id, entry, Some(author), target_program, clock, input)
            .map_err(spel_err)?;
        Ok(SpelOutput::execute(diffs, vec![]))
    }

    /// Transfer authorship of a third-party entry. Both parties must sign.
    #[instruction]
    pub fn transfer_authorship(
        ctx: ProgramContext,
        #[account(mut)] entry: AccountWithMetadata,
        #[account(signer)] current_author: AccountWithMetadata,
        #[account(signer)] new_author: AccountWithMetadata,
        new_author_name: String,
    ) -> SpelResult {
        let diffs = registry_program::transfer_authorship(
            ctx.self_account_id, entry, current_author, new_author, new_author_name,
        )
        .map_err(spel_err)?;
        Ok(SpelOutput::execute(diffs, vec![]))
    }
}
