//! C ABI for the LEZ Program Registry SDK.
//!
//! Every function takes NUL-terminated UTF-8 JSON and returns a newly
//! allocated JSON string `{"ok": <value>}` or `{"error": "<message>"}`, to be
//! released with `registry_free_string`. Config JSON:
//! `{"sequencer_url": "...", "storage_url": "...", "registry_id": "<base58>"}`.

use lez_registry_sdk::{
    parse_account_id,
    write::{Signer, Update},
    Config, EntryKind, Query, Registry,
};
use registry_core::EntryInput;
use serde::Deserialize;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};

#[derive(Deserialize)]
struct Cfg {
    sequencer_url: String,
    storage_url: String,
    registry_id: String,
    #[serde(default)]
    spel_bin: Option<String>,
    #[serde(default)]
    idl_path: Option<String>,
    #[serde(default)]
    fee_payer: Option<String>,
}

fn arg(p: *const c_char) -> Result<String, String> {
    if p.is_null() {
        return Err("null argument".into());
    }
    // SAFETY: callers pass NUL-terminated strings (checked non-null above).
    unsafe { CStr::from_ptr(p) }.to_str().map(str::to_owned).map_err(|e| e.to_string())
}

fn out(r: Result<Value, String>) -> *mut c_char {
    let v = match r {
        Ok(v) => json!({ "ok": v }),
        Err(e) => json!({ "error": e }),
    };
    CString::new(v.to_string()).map_or(std::ptr::null_mut(), CString::into_raw)
}

fn registry(cfg: &str) -> Result<(Registry, Cfg), String> {
    let c: Cfg = serde_json::from_str(cfg).map_err(|e| format!("config: {e}"))?;
    let registry_id = parse_account_id(&c.registry_id).map_err(|e| e.to_string())?;
    let reg = Registry::new(Config {
        sequencer_url: c.sequencer_url.clone(),
        storage_url: c.storage_url.clone(),
        registry_id,
    });
    Ok((reg, c))
}

fn signer(c: &Cfg) -> Result<Signer, String> {
    Ok(Signer {
        spel_bin: c.spel_bin.clone().unwrap_or_else(|| "spel".into()).into(),
        idl_path: c.idl_path.clone().ok_or("config.idl_path (registry IDL) is required for writes")?.into(),
        fee_payer: c.fee_payer.clone(),
    })
}

fn to_value(v: impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}

/// Resolves a program id to its entries with IDL status (`IdlStatus`).
#[no_mangle]
pub extern "C" fn registry_resolve(cfg: *const c_char, program_id: *const c_char) -> *mut c_char {
    out((|| {
        let (reg, _) = registry(&arg(cfg)?)?;
        let id = parse_account_id(&arg(program_id)?).map_err(|e| e.to_string())?;
        to_value(reg.resolve(&id).map_err(|e| e.to_string())?)
    })())
}

/// Enumerates every entry.
#[no_mangle]
pub extern "C" fn registry_list(cfg: *const c_char) -> *mut c_char {
    out((|| {
        let (reg, _) = registry(&arg(cfg)?)?;
        to_value(reg.list().map_err(|e| e.to_string())?)
    })())
}

/// Searches entries. Query JSON: `{"name": "...", "author": "...", "kind": "deployer"|"third_party"}`.
#[no_mangle]
pub extern "C" fn registry_search(cfg: *const c_char, query: *const c_char) -> *mut c_char {
    out((|| {
        let (reg, _) = registry(&arg(cfg)?)?;
        let q: Value = serde_json::from_str(&arg(query)?).map_err(|e| e.to_string())?;
        let s = |k: &str| q.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned);
        let kind = match s("kind").as_deref() {
            Some("deployer") => Some(EntryKind::Deployer),
            Some("third_party" | "third-party") => Some(EntryKind::ThirdParty),
            _ => None,
        };
        to_value(reg.search(&Query { name: s("name"), author: s("author"), kind }).map_err(|e| e.to_string())?)
    })())
}

#[derive(Deserialize)]
struct RegisterReq {
    program_id: String,
    /// Set for third-party entries.
    #[serde(default)]
    author: Option<String>,
    #[serde(flatten)]
    input: EntryInputJson,
}

#[derive(Deserialize)]
struct EntryInputJson {
    name: String,
    version: String,
    author_name: String,
    description: String,
    idl_pointer: String,
    code_pointer: String,
    #[serde(default)]
    source_url: String,
    commit: String,
    build_config: String,
}

/// Registers an entry via `spel` and the user's LEZ wallet. Returns the entry id.
#[no_mangle]
pub extern "C" fn registry_register(cfg: *const c_char, request: *const c_char) -> *mut c_char {
    out((|| {
        let (reg, c) = registry(&arg(cfg)?)?;
        let r: RegisterReq = serde_json::from_str(&arg(request)?).map_err(|e| e.to_string())?;
        let program = parse_account_id(&r.program_id).map_err(|e| e.to_string())?;
        let i = r.input;
        let input = EntryInput {
            name: i.name,
            version: i.version,
            author_name: i.author_name,
            description: i.description,
            idl_pointer: i.idl_pointer,
            code_pointer: i.code_pointer,
            source_url: i.source_url,
            commit: i.commit,
            build_config: i.build_config,
        };
        let s = signer(&c)?;
        let id = match r.author {
            None => s.register_deployer(&reg, &program, &input),
            Some(a) => {
                let a = parse_account_id(&a).map_err(|e| e.to_string())?;
                s.register_third_party(&reg, &program, &a, &input)
            }
        }
        .map_err(|e| e.to_string())?;
        Ok(json!(id.to_string()))
    })())
}

#[derive(Deserialize)]
struct UpdateReq {
    entry_id: String,
    #[serde(default)]
    author: Option<String>,
    version: String,
    description: String,
    idl_pointer: String,
    code_pointer: String,
    #[serde(default)]
    source_url: String,
    commit: String,
    build_config: String,
}

/// Updates an entry (author only; `author` omitted for deployer entries).
#[no_mangle]
pub extern "C" fn registry_update(cfg: *const c_char, request: *const c_char) -> *mut c_char {
    out((|| {
        let (reg, c) = registry(&arg(cfg)?)?;
        let r: UpdateReq = serde_json::from_str(&arg(request)?).map_err(|e| e.to_string())?;
        let entry = parse_account_id(&r.entry_id).map_err(|e| e.to_string())?;
        let author = r.author.as_deref().map(parse_account_id).transpose().map_err(|e| e.to_string())?;
        let u = Update {
            version: r.version,
            description: r.description,
            idl_pointer: r.idl_pointer,
            code_pointer: r.code_pointer,
            source_url: r.source_url,
            commit: r.commit,
            build_config: r.build_config,
        };
        signer(&c)?.update(&reg, &entry, author.as_ref(), &u).map_err(|e| e.to_string())?;
        Ok(json!(true))
    })())
}

/// Releases a string returned by this library.
#[no_mangle]
pub extern "C" fn registry_free_string(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: `s` was produced by `CString::into_raw` in this library.
        drop(unsafe { CString::from_raw(s) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(f: impl FnOnce() -> *mut c_char) -> Value {
        let p = f();
        // SAFETY: p comes from `out`.
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        registry_free_string(p);
        serde_json::from_str(&s).unwrap_or_default()
    }

    #[test]
    fn errors_are_json() {
        let bad = CString::new("{}").unwrap_or_default();
        let v = call(|| registry_list(bad.as_ptr()));
        assert!(v["error"].as_str().is_some_and(|e| e.contains("config")), "{v}");
        let v = call(|| registry_list(std::ptr::null()));
        assert_eq!(v["error"], "null argument");
    }
}
