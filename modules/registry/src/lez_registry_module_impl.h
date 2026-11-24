#pragma once

#include <string>

#include <logos_json.h>
#include <logos_module_context.h>

// Universal Logos Core Module over the registry SDK (registry_ffi).
// Every method returns the SDK result as a JSON string:
// {"ok": ...} on success or {"error": "..."} on failure. Reads never fail
// because an IDL is missing: entries carry idl.status == "unavailable".
class LezRegistryModuleImpl : public LogosModuleContext {
public:
    LezRegistryModuleImpl() = default;
    ~LezRegistryModuleImpl() = default;

    // {"sequencer_url", "storage_url", "registry_id", optional "spel_bin",
    //  "idl_path", "fee_payer"}. Defaults come from LEZ_SEQUENCER_URL,
    // LEZ_STORAGE_URL, LEZ_REGISTRY_ID when not configured.
    std::string configure(const std::string& config_json);
    std::string config();

    std::string resolve(const std::string& program_id);
    std::string list();
    std::string search(const std::string& name, const std::string& author, const std::string& kind);
    std::string registerEntry(const std::string& request_json);
    std::string updateEntry(const std::string& request_json);

private:
    std::string m_config;
    std::string effectiveConfig() const;
};
