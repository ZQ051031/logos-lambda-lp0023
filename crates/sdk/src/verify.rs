//! Source verification: rebuild the recorded source and compare its ImageID
//! with the one the deployed program header holds. A match can be recorded
//! as "locally verified" in a per-user file; nothing is published.

use crate::{image_id_hex, Error, RegistryEntry, Result};
use lee_core::program::ProgramHeader;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Reproducible build configuration stored (as JSON) in `RegistryEntry::build_config`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildConfig {
    /// `risc0-docker`: `cargo risczero build`, which runs the pinned
    /// `risczero/risc0-guest-builder` image for a deterministic ELF.
    pub builder: String,
    /// `cargo-risczero` / `risc0-zkvm` version the source pins.
    pub risc0: String,
    /// Guest manifest path inside the source archive.
    pub manifest: String,
    /// Guest binary name (`[[bin]] name`).
    pub binary: String,
}

impl BuildConfig {
    pub fn parse(s: &str) -> Result<Self> {
        serde_json::from_str(s).map_err(|e| Error::Decode(format!("build_config: {e}")))
    }

    /// Where `cargo risczero build` leaves the deployable `.bin`.
    #[must_use]
    pub fn output_path(&self, src: &Path) -> PathBuf {
        let guest = src.join(&self.manifest);
        let dir = guest.parent().unwrap_or(src);
        dir.join("target/riscv32im-risc0-zkvm-elf/docker")
            .join(format!("{}.bin", self.binary))
    }
}

/// Unpacks a `git archive --format=tar.gz` source archive.
pub fn unpack(archive: &[u8], dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest).map_err(|e| Error::Other(e.to_string()))?;
    let tarball = dest.join(".source.tar.gz");
    std::fs::write(&tarball, archive).map_err(|e| Error::Other(e.to_string()))?;
    let status = Command::new("tar")
        .args(["--no-same-owner", "-xzf"])
        .arg(&tarball)
        .arg("-C")
        .arg(dest)
        .status()
        .map_err(|e| Error::Other(format!("tar: {e}")))?;
    let _ = std::fs::remove_file(&tarball);
    if !status.success() {
        return Err(Error::Other("source archive failed to unpack".into()));
    }
    Ok(())
}

/// Runs the recorded build and returns the built `.bin` path.
pub fn rebuild(src: &Path, cfg: &BuildConfig) -> Result<PathBuf> {
    if cfg.builder != "risc0-docker" {
        return Err(Error::Other(format!(
            "unsupported builder `{}`",
            cfg.builder
        )));
    }
    let manifest = src.join(&cfg.manifest);
    if !manifest.starts_with(src) || cfg.manifest.contains("..") {
        return Err(Error::Other("manifest path escapes the source tree".into()));
    }
    let status = Command::new("cargo")
        .args(["risczero", "build", "--manifest-path"])
        .arg(&manifest)
        .current_dir(src)
        .status()
        .map_err(|e| Error::Other(format!("cargo risczero: {e}")))?;
    if !status.success() {
        return Err(Error::Other("guest build failed".into()));
    }
    let out = cfg.output_path(src);
    if !out.exists() {
        return Err(Error::Other(format!(
            "build did not produce {}",
            out.display()
        )));
    }
    Ok(out)
}

/// ImageID of a RISC Zero program binary (`.bin`, R0BF) as `[u32; 8]`.
pub fn image_id_of(bin: &[u8]) -> Result<[u32; 8]> {
    let digest =
        risc0_binfmt::compute_image_id(bin).map_err(|e| Error::Decode(format!("image id: {e}")))?;
    let mut words = [0u32; 8];
    words.copy_from_slice(digest.as_words());
    Ok(words)
}

/// Outcome of comparing a rebuild with chain state.
#[derive(Clone, Debug, Serialize)]
pub struct Comparison {
    pub built_image_id: String,
    pub header_image_id: String,
    pub entry_image_id: String,
    pub matches: bool,
}

/// Compares a rebuilt binary against the live program header (the ground
/// truth) and the ImageID the entry recorded.
#[must_use]
pub fn compare(built: [u32; 8], header: &ProgramHeader, entry: &RegistryEntry) -> Comparison {
    Comparison {
        built_image_id: image_id_hex(&built),
        header_image_id: image_id_hex(&header.image_id),
        entry_image_id: image_id_hex(&entry.image_id),
        matches: built == header.image_id,
    }
}

/// A local, per-user "verified" mark.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalVerification {
    pub image_id: String,
    pub entry_id: String,
    pub commit: String,
    pub code_pointer: String,
    pub verified_at_unix: u64,
}

/// `program_id -> mark`, stored at `$LEZ_REGISTRY_HOME/verified.json`
/// (default `~/.config/lez-registry/verified.json`).
#[derive(Default, Serialize, Deserialize)]
pub struct LocalVerifications {
    pub programs: BTreeMap<String, LocalVerification>,
}

#[must_use]
pub fn home_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("LEZ_REGISTRY_HOME") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    home.join(".config/lez-registry")
}

impl LocalVerifications {
    #[must_use]
    pub fn path() -> PathBuf {
        home_dir().join("verified.json")
    }

    pub fn load() -> Result<Self> {
        match std::fs::read(Self::path()) {
            Ok(b) => serde_json::from_slice(&b).map_err(|e| Error::Decode(e.to_string())),
            Err(_) => Ok(Self::default()),
        }
    }

    pub fn save(&self) -> Result<()> {
        let p = Self::path();
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| Error::Other(e.to_string()))?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(|e| Error::Other(e.to_string()))?;
        std::fs::write(p, json).map_err(|e| Error::Other(e.to_string()))
    }

    /// Valid only while the deployed ImageID is still the one that was verified.
    #[must_use]
    pub fn is_verified(&self, program_id: &str, current_image_id: &str) -> bool {
        self.programs
            .get(program_id)
            .is_some_and(|m| m.image_id == current_image_id)
    }
}
