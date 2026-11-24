#include "lez_registry_module_impl.h"

#include <cstdlib>
#include <functional>

#include <nlohmann/json.hpp>

#include "registry_ffi.h"

using json = nlohmann::json;

namespace {

std::string env(const char* name, const char* fallback) {
    const char* v = std::getenv(name);
    return (v && *v) ? v : fallback;
}

std::string take(char* raw) {
    if (!raw) return R"({"error":"registry_ffi returned null"})";
    std::string s(raw);
    registry_free_string(raw);
    return s;
}

std::string error(const std::string& message) {
    return json{{"error", message}}.dump();
}

}  // namespace

std::string LezRegistryModuleImpl::effectiveConfig() const {
    json c = json::object();
    if (!m_config.empty()) c = json::parse(m_config, nullptr, false);
    if (!c.is_object()) c = json::object();
    if (!c.contains("sequencer_url")) c["sequencer_url"] = env("LEZ_SEQUENCER_URL", "http://127.0.0.1:3040");
    if (!c.contains("storage_url"))
        c["storage_url"] = env("LEZ_STORAGE_URL", "http://127.0.0.1:8080/api/storage/v1");
    if (!c.contains("registry_id")) c["registry_id"] = env("LEZ_REGISTRY_ID", "");
    if (!c.contains("idl_path") && std::getenv("LEZ_REGISTRY_IDL")) c["idl_path"] = env("LEZ_REGISTRY_IDL", "");
    if (!c.contains("fee_payer") && std::getenv("LEZ_FEE_PAYER")) c["fee_payer"] = env("LEZ_FEE_PAYER", "");
    return c.dump();
}

std::string LezRegistryModuleImpl::configure(const std::string& config_json) {
    const json c = json::parse(config_json, nullptr, false);
    if (!c.is_object()) return error("config must be a JSON object");
    m_config = c.dump();
    return json{{"ok", json::parse(effectiveConfig())}}.dump();
}

std::string LezRegistryModuleImpl::config() {
    return json{{"ok", json::parse(effectiveConfig())}}.dump();
}

std::string LezRegistryModuleImpl::resolve(const std::string& program_id) {
    return take(registry_resolve(effectiveConfig().c_str(), program_id.c_str()));
}

std::string LezRegistryModuleImpl::list() {
    return take(registry_list(effectiveConfig().c_str()));
}

std::string LezRegistryModuleImpl::search(const std::string& name, const std::string& author,
                                          const std::string& kind) {
    const std::string q = json{{"name", name}, {"author", author}, {"kind", kind}}.dump();
    return take(registry_search(effectiveConfig().c_str(), q.c_str()));
}

std::string LezRegistryModuleImpl::registerEntry(const std::string& request_json) {
    return take(registry_register(effectiveConfig().c_str(), request_json.c_str()));
}

std::string LezRegistryModuleImpl::updateEntry(const std::string& request_json) {
    return take(registry_update(effectiveConfig().c_str(), request_json.c_str()));
}
