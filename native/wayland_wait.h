// SPDX-License-Identifier: GPL-3.0-or-later
#ifndef SCROLLMASTER_WAYLAND_WAIT_H
#define SCROLLMASTER_WAYLAND_WAIT_H
#include <stdint.h>
struct wl_display;
uint64_t scrollmaster_wayland_now_ms(void);
int scrollmaster_wayland_roundtrip_until(struct wl_display *display, uint64_t deadline_ms);
#endif
