// SPDX-License-Identifier: GPL-3.0-or-later
// Minimal client for zwp_virtual_keyboard_v1. Protocol ABI, no compositor config.
#define _GNU_SOURCE
#include <wayland-client.h>
#include <xkbcommon/xkbcommon.h>
#include <sys/mman.h>
#include <unistd.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "wayland_wait.h"

#define KEYBOARD_TIMEOUT_MS 2000
static uint64_t operation_deadline(void) {
    uint64_t now = scrollmaster_wayland_now_ms();
    return now ? now + KEYBOARD_TIMEOUT_MS : 0;
}

static const struct wl_interface keyboard_interface;
static const struct wl_interface *create_types[] = { &wl_seat_interface, &keyboard_interface };
static const struct wl_message keyboard_requests[] = {
    {"keymap", "uhu", NULL}, {"key", "uuu", NULL},
    {"modifiers", "uuuu", NULL}, {"destroy", "", NULL}
};
static const struct wl_interface keyboard_interface = {
    "zwp_virtual_keyboard_v1", 1, 4, keyboard_requests, 0, NULL
};
static const struct wl_message manager_requests[] = {
    {"create_virtual_keyboard", "on", create_types}
};
static const struct wl_interface manager_interface = {
    "zwp_virtual_keyboard_manager_v1", 1, 1, manager_requests, 0, NULL
};
struct keyboard {
    struct wl_display *display;
    struct wl_registry *registry;
    struct wl_seat *seat;
    struct wl_proxy *manager, *keyboard;
    int ready;
    unsigned mask, pending_key;
};
static void global(void *data, struct wl_registry *registry, uint32_t name,
                   const char *interface, uint32_t version) {
    (void)version;
    struct keyboard *k = data;
    if (!strcmp(interface, "wl_seat") && !k->seat)
        k->seat = wl_registry_bind(registry, name, &wl_seat_interface, 1);
    if (!strcmp(interface, manager_interface.name) && !k->manager)
        k->manager = wl_registry_bind(registry, name, &manager_interface, 1);
}
static void removed(void *data, struct wl_registry *registry, uint32_t name) {
    (void)data; (void)registry; (void)name;
}
static const struct wl_registry_listener registry_listener = { global, removed };

static int state_until(void *handle, unsigned mask, uint64_t deadline) {
    struct keyboard *k = handle;
    if (!k || !k->keyboard || (mask & ~77u)) return -1;
    // Verified indices in our uploaded keymap: Shift=1, Control=4, Alt=8, Logo=64.
    wl_proxy_marshal_flags(k->keyboard, 2, NULL, 1, 0, mask, 0u, 0u, 0u);
    if (scrollmaster_wayland_roundtrip_until(k->display, deadline) < 0) return -1;
    k->mask = mask;
    return 0;
}
int scrollmaster_keyboard_state(void *handle, unsigned mask) {
    return state_until(handle, mask, operation_deadline());
}
static unsigned timestamp(void) {
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    return (unsigned)((uint64_t)now.tv_sec * 1000 + now.tv_nsec / 1000000);
}
int scrollmaster_keyboard_action_valid(unsigned key, unsigned modifiers) {
    // Letters, digits, F1–F12, navigation and a small set of named editing keys.
    // Excludes modifier keycodes, locks, power/session and arbitrary evdev input.
    int normal = (key >= 1 && key <= 11) || key == 14 || key == 15 ||
        (key >= 16 && key <= 25) || key == 28 || (key >= 30 && key <= 38) ||
        (key >= 44 && key <= 50) || key == 57 || (key >= 59 && key <= 68) ||
        key == 87 || key == 88 || (key >= 102 && key <= 111);
    int media = ((key == 113 || key == 114 || key == 115 ||
                  key == 163 || key == 164 || key == 165) && modifiers == 0);
    return (normal && !(modifiers & ~77u)) || media;
}
int scrollmaster_keyboard_action(void *handle, unsigned key, unsigned modifiers) {
    struct keyboard *k = handle;
    if (!k || !k->ready || !scrollmaster_keyboard_action_valid(key, modifiers)) return -1;
    unsigned previous_mask = k->mask;
    uint64_t deadline = operation_deadline();
    if (state_until(k, modifiers, deadline) < 0) return -1;
    k->pending_key = key;
    wl_proxy_marshal_flags(k->keyboard, 1, NULL, 1, 0, timestamp(), key, 1u);
    wl_proxy_marshal_flags(k->keyboard, 1, NULL, 1, 0, timestamp(), key, 0u);
    if (scrollmaster_wayland_roundtrip_until(k->display, deadline) < 0) return -1;
    k->pending_key = 0;
    return state_until(k, previous_mask, deadline);
}
int scrollmaster_keyboard_close_checked(void *handle) {
    struct keyboard *k = handle;
    if (!k) return 0;
    int released = 0;
    if (k->keyboard) {
        if (k->pending_key)
            wl_proxy_marshal_flags(k->keyboard, 1, NULL, 1, 0, timestamp(), k->pending_key, 0u);
        if (k->ready) released = scrollmaster_keyboard_state(k, 0);
        wl_proxy_marshal_flags(k->keyboard, 3, NULL, 1, WL_MARSHAL_FLAG_DESTROY);
    }
    if (k->manager) wl_proxy_destroy(k->manager);
    if (k->seat) wl_seat_destroy(k->seat);
    if (k->registry) wl_registry_destroy(k->registry);
    if (k->display) { wl_display_flush(k->display); wl_display_disconnect(k->display); }
    free(k);
    return released;
}
void scrollmaster_keyboard_close(void *handle) {
    (void)scrollmaster_keyboard_close_checked(handle);
}
void *scrollmaster_keyboard_open(void) {
    struct keyboard *k = calloc(1, sizeof(*k));
    struct xkb_context *context = NULL;
    struct xkb_keymap *map = NULL;
    char *text = NULL;
    int fd = -1;
    uint64_t deadline = operation_deadline();
    if (!k) return NULL;
    k->display = wl_display_connect(NULL);
    if (!k->display) goto fail;
    k->registry = wl_display_get_registry(k->display);
    if (!k->registry) goto fail;
    wl_registry_add_listener(k->registry, &registry_listener, k);
    if (scrollmaster_wayland_roundtrip_until(k->display, deadline) < 0 || !k->manager || !k->seat) goto fail;
    k->keyboard = wl_proxy_marshal_flags(k->manager, 0, &keyboard_interface, 1, 0, k->seat, NULL);
    if (!k->keyboard) goto fail;
    context = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    if (!context) goto fail;
    struct xkb_rule_names names = {.layout = "us"};
    map = xkb_keymap_new_from_names(context, &names, XKB_KEYMAP_COMPILE_NO_FLAGS);
    if (!map) goto fail;
    if (xkb_keymap_mod_get_index(map, XKB_MOD_NAME_SHIFT) != 0 ||
        xkb_keymap_mod_get_index(map, XKB_MOD_NAME_CTRL) != 2 ||
        xkb_keymap_mod_get_index(map, XKB_MOD_NAME_ALT) != 3 ||
        xkb_keymap_mod_get_index(map, XKB_MOD_NAME_LOGO) != 6) goto fail;
    text = xkb_keymap_get_as_string(map, XKB_KEYMAP_FORMAT_TEXT_V1);
    if (!text) goto fail;
    size_t size = strlen(text) + 1;
    fd = memfd_create("scrollmaster-modifiers", MFD_CLOEXEC);
    if (fd < 0 || write(fd, text, size) != (ssize_t)size) goto fail;
    wl_proxy_marshal_flags(k->keyboard, 0, NULL, 1, 0, 1u, fd, (unsigned)size);
    k->ready = 1;
    close(fd); fd = -1;
    free(text); text = NULL;
    xkb_keymap_unref(map); map = NULL;
    xkb_context_unref(context); context = NULL;
    if (state_until(k, 0, deadline) < 0) goto fail;
    return k;
fail:
    if (fd >= 0) close(fd);
    free(text);
    if (map) xkb_keymap_unref(map);
    if (context) xkb_context_unref(context);
    scrollmaster_keyboard_close(k);
    return NULL;
}
