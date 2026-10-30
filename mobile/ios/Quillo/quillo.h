// The one call the app makes into the Quillo runtime (mobile/quillo-mobile).
#include <stddef.h>
// `device` is a JSON object of what the app knows of the device, for platform().
int quillo_start(const char *bundle, const char *data, const char *main, const char *device, char *out, size_t len);
