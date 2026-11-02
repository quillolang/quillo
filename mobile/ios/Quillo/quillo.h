// The calls the app makes into the Quillo runtime (mobile/quillo-mobile).
#include <stddef.h>
#include <stdint.h>
// The old way, a server on 127.0.0.1 (kept for apps built before the scheme).
int quillo_start(const char *bundle, const char *data, const char *main, const char *device, char *out, size_t len);
// The program in the app's process, with no socket (FEEDBACK39.md). `device` is a JSON object of
// what the app knows of the device, for platform(). 0, or 1 with the reason in `err`.
int quillo_open(const char *bundle, const char *data, const char *main, const char *device, char *err, size_t len);
typedef struct { int32_t status; char *headers; uint8_t *body; size_t len; } QuilloAnswer;
// One request of the web view's, answered whole; `headers_json` and the answer's headers are
// JSON objects. Free the answer with quillo_free_answer.
QuilloAnswer *quillo_request(const char *method, const char *url, const char *headers_json, const uint8_t *body, size_t len);
void quillo_free_answer(QuilloAnswer *a);
// The page's bridge: a message from it, and the next one for it (JSON, or NULL after timeout_ms).
void quillo_bridge(const char *message);
char *quillo_bridge_next(uint32_t timeout_ms);
void quillo_free_text(char *t);
// The script to run before every page.
char *quillo_shell_js(const char *post, const char *bodies);
void quillo_suspend(void);
void quillo_resume(void);
