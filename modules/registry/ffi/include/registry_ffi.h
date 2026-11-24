/* C ABI of the LEZ Program Registry SDK (modules/registry/ffi/src/lib.rs).
 * All arguments are NUL-terminated UTF-8 JSON. Results are JSON
 * {"ok": ...} or {"error": "..."} and must be freed with registry_free_string. */
#ifndef REGISTRY_FFI_H
#define REGISTRY_FFI_H
#ifdef __cplusplus
extern "C" {
#endif
char *registry_resolve(const char *config_json, const char *program_id);
char *registry_list(const char *config_json);
char *registry_search(const char *config_json, const char *query_json);
char *registry_register(const char *config_json, const char *request_json);
char *registry_update(const char *config_json, const char *request_json);
void registry_free_string(char *s);
#ifdef __cplusplus
}
#endif
#endif
