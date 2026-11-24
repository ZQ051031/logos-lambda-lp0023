//! Writes go through the `spel` CLI (SPEL's IDL-driven transaction builder,
//! signing with the user's LEZ wallet at `LEE_WALLET_HOME_DIR`). The SDK
//! never handles private keys.
//!
//! `spel` reports "Transaction confirmed" even when the program rejected the
//! instruction, so every write is followed by a chain read that checks the
//! expected effect.

use crate::{pda, Error, Registry, Result};
use lee_core::account::AccountId;
use registry_core::{EntryInput, CLOCK_ACCOUNT_ID};
use std::path::PathBuf;
use std::process::Command;

/// How to invoke `spel`.
#[derive(Clone, Debug)]
pub struct Signer {
    pub spel_bin: PathBuf,
    pub idl_path: PathBuf,
    pub fee_payer: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Update {
    pub version: String,
    pub description: String,
    pub idl_pointer: String,
    pub code_pointer: String,
    pub source_url: String,
    pub commit: String,
    pub build_config: String,
}

impl Signer {
    fn run(&self, reg: &Registry, instruction: &str, args: &[(&str, String)]) -> Result<String> {
        let mut cmd = Command::new(&self.spel_bin);
        cmd.env("SEQUENCER_URL", &reg.config.sequencer_url)
            .arg("--idl")
            .arg(&self.idl_path)
            .arg("-p")
            .arg(reg.config.registry_id.to_string());
        if let Some(fp) = &self.fee_payer {
            cmd.arg("--fee-payer").arg(fp);
        }
        cmd.arg("--").arg(instruction);
        for (k, v) in args {
            cmd.arg(format!("--{k}")).arg(v);
        }
        let out = cmd
            .output()
            .map_err(|e| Error::Other(format!("spel: {e}")))?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        if !out.status.success() {
            return Err(Error::Other(format!("spel {instruction} failed:\n{text}")));
        }
        Ok(text)
    }

    pub fn initialize(&self, reg: &Registry, payer: &AccountId) -> Result<()> {
        self.run(reg, "initialize", &[("payer", payer.to_string())])?;
        reg.state().map(|_| ())
    }

    fn entry_args(index: u64, input: &EntryInput) -> Vec<(&'static str, String)> {
        vec![
            ("index", index.to_string()),
            ("slot-key", hex::encode(registry_core::seed_from_u64(index))),
            ("name", input.name.clone()),
            ("version", input.version.clone()),
            ("author-name", input.author_name.clone()),
            ("description", input.description.clone()),
            ("idl-pointer", input.idl_pointer.clone()),
            ("code-pointer", input.code_pointer.clone()),
            ("source-url", input.source_url.clone()),
            ("commit", input.commit.clone()),
            ("build-config", input.build_config.clone()),
            ("clock", CLOCK_ACCOUNT_ID.to_string()),
        ]
    }

    /// Registers the deployer entry; `program` must be a header key in the wallet.
    pub fn register_deployer(
        &self,
        reg: &Registry,
        program: &AccountId,
        input: &EntryInput,
    ) -> Result<AccountId> {
        let index = reg.state()?.entry_count;
        let mut args = Self::entry_args(index, input);
        args.push(("target-program", program.to_string()));
        let log = self.run(reg, "register_deployer_entry", &args)?;
        let id = pda::deployer_entry(&reg.config.registry_id, program);
        match reg.entry_at(&id)? {
            Some(v) if v.verified_deployer => Ok(id),
            _ => Err(Error::Other(format!(
                "deployer entry was not created (rejected on-chain?)\n{log}"
            ))),
        }
    }

    pub fn register_third_party(
        &self,
        reg: &Registry,
        program: &AccountId,
        author: &AccountId,
        input: &EntryInput,
    ) -> Result<AccountId> {
        let index = reg.state()?.entry_count;
        let mut args = Self::entry_args(index, input);
        args.push(("target-program", program.to_string()));
        args.push(("author", author.to_string()));
        let log = self.run(reg, "register_third_party_entry", &args)?;
        let id = pda::third_party_entry(&reg.config.registry_id, program, author);
        match reg.entry_at(&id)? {
            Some(_) => Ok(id),
            None => Err(Error::Other(format!(
                "third-party entry was not created (rejected on-chain?)\n{log}"
            ))),
        }
    }

    /// Updates an entry. `author` is `None` for deployer entries (the header signs).
    pub fn update(
        &self,
        reg: &Registry,
        entry_id: &AccountId,
        author: Option<&AccountId>,
        u: &Update,
    ) -> Result<()> {
        let view = reg
            .entry_at(entry_id)?
            .ok_or_else(|| Error::Other("no such entry".into()))?;
        let mut args = vec![
            ("version", u.version.clone()),
            ("description", u.description.clone()),
            ("idl-pointer", u.idl_pointer.clone()),
            ("code-pointer", u.code_pointer.clone()),
            ("source-url", u.source_url.clone()),
            ("commit", u.commit.clone()),
            ("build-config", u.build_config.clone()),
            ("target-program", view.program_id.clone()),
            ("clock", CLOCK_ACCOUNT_ID.to_string()),
        ];
        let ix = match author {
            None => "update_deployer_entry",
            Some(a) => {
                args.push(("entry", entry_id.to_string()));
                args.push(("author", a.to_string()));
                "update_third_party_entry"
            }
        };
        let log = self.run(reg, ix, &args)?;
        let after = reg
            .entry_at(entry_id)?
            .ok_or_else(|| Error::Other("entry vanished".into()))?;
        if after.entry.version != u.version || after.entry.idl_pointer != u.idl_pointer {
            return Err(Error::Other(format!(
                "update was not applied (rejected on-chain?)\n{log}"
            )));
        }
        Ok(())
    }

    /// Transfers a third-party entry; both accounts must be in the wallet.
    pub fn transfer(
        &self,
        reg: &Registry,
        entry_id: &AccountId,
        current: &AccountId,
        new_author: &AccountId,
        new_author_name: &str,
    ) -> Result<()> {
        let args = vec![
            ("entry", entry_id.to_string()),
            ("current-author", current.to_string()),
            ("new-author", new_author.to_string()),
            ("new-author-name", new_author_name.to_string()),
        ];
        let log = self.run(reg, "transfer_authorship", &args)?;
        let after = reg
            .entry_at(entry_id)?
            .ok_or_else(|| Error::Other("entry vanished".into()))?;
        if after.entry.author != *new_author {
            return Err(Error::Other(format!(
                "transfer was not applied (rejected on-chain?)\n{log}"
            )));
        }
        Ok(())
    }
}
