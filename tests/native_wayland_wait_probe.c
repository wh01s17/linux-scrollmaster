// SPDX-License-Identifier: GPL-3.0-or-later
// Real libwayland-client against an anonymous synthetic protocol peer.
#include <wayland-client.h>
#include "../native/wayland_wait.h"
int probe_wait(int fd, unsigned milliseconds, unsigned rounds) {
    struct wl_display *display = wl_display_connect_to_fd(fd);
    if (!display) return -1;
    uint64_t deadline = scrollmaster_wayland_now_ms() + milliseconds;
    int result = 0;
    for (unsigned i = 0; i < rounds; ++i) {
        result = scrollmaster_wayland_roundtrip_until(display, deadline);
        if (result < 0) break;
    }
    wl_display_disconnect(display);
    return result;
}
