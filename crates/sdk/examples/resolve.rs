//! Resolve a program id to its registry metadata and IDL.
//!
//! ```text
//! LEZ_REGISTRY_ID=<registry> cargo run -p lez-registry-sdk --example resolve -- <program-id>
//! ```

use lez_registry_sdk::{parse_account_id, Config, IdlStatus, Registry};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_owned());
    let registry = Registry::new(Config {
        sequencer_url: env("LEZ_SEQUENCER_URL", "http://127.0.0.1:3040"),
        storage_url: env("LEZ_STORAGE_URL", "http://127.0.0.1:8080/api/storage/v1"),
        registry_id: parse_account_id(&std::env::var("LEZ_REGISTRY_ID")?)?,
    });
    let program_id = parse_account_id(
        &std::env::args()
            .nth(1)
            .ok_or("usage: resolve <program-id>")?,
    )?;

    for r in registry.resolve(&program_id)? {
        let kind = if r.view.verified_deployer {
            "deployer (signed by program header)"
        } else {
            "third-party"
        };
        println!(
            "{} {} [{kind}] by {} ({})",
            r.view.entry.name, r.view.entry.version, r.view.entry.author_name, r.view.author
        );
        println!(
            "  registered at {} ms, image id {}",
            r.view.entry.registered_at, r.view.image_id
        );
        match r.idl {
            IdlStatus::Available { idl } => {
                let n = idl["instructions"].as_array().map_or(0, Vec::len);
                println!("  IDL: {} with {n} instructions", idl["name"]);
            }
            IdlStatus::Unavailable { reason } => println!("  IDL unavailable ({reason})"),
        }
    }
    Ok(())
}
