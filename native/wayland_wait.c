// SPDX-License-Identifier: GPL-3.0-or-later
// Single-reader, absolute-deadline synchronization for our private Wayland display.
#define _GNU_SOURCE
#include <wayland-client.h>
#include <errno.h>
#include <limits.h>
#include <poll.h>
#include <time.h>
#include "wayland_wait.h"

uint64_t scrollmaster_wayland_now_ms(void) {
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) < 0) return 0;
    return (uint64_t)now.tv_sec * 1000 + (uint64_t)now.tv_nsec / 1000000;
}
static void sync_done(void *data, struct wl_callback *callback, uint32_t serial) {
    (void)callback; (void)serial;
    *(int *)data = 1;
}
static const struct wl_callback_listener sync_listener = {sync_done};
static int remaining(uint64_t deadline) {
    uint64_t now = scrollmaster_wayland_now_ms();
    if (!now || now >= deadline) { errno = ETIMEDOUT; return -1; }
    return deadline - now > INT_MAX ? INT_MAX : (int)(deadline - now);
}
int scrollmaster_wayland_roundtrip_until(struct wl_display *display, uint64_t deadline) {
    if (!display || remaining(deadline) < 0) return -1;
    struct wl_callback *sync = wl_display_sync(display);
    if (!sync) return -1;
    int done = 0, result = -1;
    if (wl_callback_add_listener(sync, &sync_listener, &done) < 0) goto out;
    while (!done) {
        if (remaining(deadline) < 0) goto out;
        while (wl_display_prepare_read(display) != 0) {
            if (remaining(deadline) < 0 || wl_display_dispatch_pending(display) < 0) goto out;
            if (done) goto completed;
        }
        int flushed = wl_display_flush(display);
        if (flushed < 0 && errno != EAGAIN) { wl_display_cancel_read(display); goto out; }
        int timeout = remaining(deadline);
        if (timeout < 0) { wl_display_cancel_read(display); goto out; }
        struct pollfd fd = {.fd = wl_display_get_fd(display), .events = POLLIN};
        if (flushed < 0) fd.events |= POLLOUT;
        int status = poll(&fd, 1, timeout);
        if (status < 0) {
            int interrupted = errno == EINTR;
            wl_display_cancel_read(display);
            if (interrupted) continue;
            goto out;
        }
        if (remaining(deadline) < 0 || status == 0) { wl_display_cancel_read(display); errno = ETIMEDOUT; goto out; }
        if (fd.revents & POLLIN) {
            // read_events consumes the prepared read, including on error.
            if (wl_display_read_events(display) < 0 || wl_display_dispatch_pending(display) < 0) goto out;
        } else {
            wl_display_cancel_read(display);
            if (fd.revents & (POLLHUP | POLLERR | POLLNVAL)) goto out;
        }
    }
completed:
    if (remaining(deadline) >= 0) result = 0;
out:
    wl_callback_destroy(sync);
    return result;
}
