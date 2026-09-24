#include <os/log.h>

// Use the public SDK macro: clang generates the required os_log format metadata.
// Rust must not construct the private _os_log_impl buffer ABI itself.
void *aow_macos_log_create(const char *category) {
    return os_log_create("org.aow", category);
}

void aow_macos_log_release(void *log) {
    os_release((os_log_t)log);
}

void aow_macos_log_write(void *log, unsigned char level, const char *message) {
    os_log_type_t type;
    switch (level) {
        case 1: type = OS_LOG_TYPE_DEBUG; break;
        case 2: type = OS_LOG_TYPE_ERROR; break;
        case 3: type = OS_LOG_TYPE_FAULT; break;
        default: type = OS_LOG_TYPE_DEFAULT; break;
    }
    // These are already-rendered diagnostic messages, including their fields.
    // Keep them readable, as with the previous text logs; never log credentials.
    os_log_with_type((os_log_t)log, type, "%{public}s", message);
}
