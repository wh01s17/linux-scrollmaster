// Synthetic transport for the actual native action implementation. No display opened.
#define _GNU_SOURCE
#include <wayland-client.h>
#include <stdarg.h>
static unsigned events[16];
static int count, trips, fail_trip, clocks;
static uint64_t deadlines[16];
static struct wl_proxy *probe_marshal(struct wl_proxy *proxy, uint32_t opcode,
        const struct wl_interface *interface, uint32_t version, uint32_t flags, ...) {
    (void)interface; (void)version; (void)flags;
    va_list args;
    va_start(args, flags);
    if (opcode == 2 && count < 16) events[count++] = 1000 + va_arg(args, unsigned);
    if (opcode == 1 && count < 16) {
        (void)va_arg(args, unsigned);
        unsigned key = va_arg(args, unsigned), state = va_arg(args, unsigned);
        events[count++] = (state ? 2000 : 3000) + key;
    }
    va_end(args);
    return proxy;
}
int scrollmaster_wayland_roundtrip_until(struct wl_display *display, uint64_t deadline) {
    (void)display;
    if (trips < 16) deadlines[trips] = deadline;
    return ++trips == fail_trip ? -1 : 0;
}
uint64_t scrollmaster_wayland_now_ms(void) { return 1000 + (uint64_t)(clocks++ * 17); }
#define wl_proxy_marshal_flags probe_marshal
#include "../native/keyboard.c"
static struct keyboard fixture;
int probe_run(unsigned key, unsigned mask, unsigned previous, int failure) {
    memset(events, 0, sizeof(events)); memset(deadlines, 0, sizeof(deadlines));
    count = trips = clocks = 0; fail_trip = failure;
    fixture = (struct keyboard){.keyboard = (struct wl_proxy *)1, .ready = 1, .mask = previous};
    return scrollmaster_keyboard_action(&fixture, key, mask);
}
unsigned probe_event(unsigned i) { return i < 16 ? events[i] : 0; }
unsigned probe_pending(void) { return fixture.pending_key; }
unsigned probe_mask(void) { return fixture.mask; }
uint64_t probe_deadline(unsigned i) { return i < 16 ? deadlines[i] : 0; }
void probe_close(void) {
    struct keyboard *copy = malloc(sizeof(*copy));
    *copy = fixture;
    scrollmaster_keyboard_close(copy);
}
int probe_close_failure(void) {
    struct keyboard *copy = malloc(sizeof(*copy));
    *copy = fixture;
    fail_trip = trips + 1;
    return scrollmaster_keyboard_close_checked(copy);
}
