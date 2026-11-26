/*
 * A Wayland client that gives a headless compositor a keyboard and a
 * pointer, for the window tests:
 *
 *   input /ABSOLUTE/PATH/OF/THE/COMPOSITOR'S/SOCKET
 *
 * It connects to that socket alone, opening it itself, and reads no
 * variable of the environment to find a compositor. It binds every
 * wl_output at version 4 and sends nothing until each has announced its
 * name: unless the compositor has at least one output and every output is
 * named `HEADLESS-` and a number, it stops with status 1 and a line on
 * standard error. An output announced after that stops it too.
 *
 * Then it makes a virtual keyboard, whose keymap is the US layout of xkb's
 * `evdev` rules, and a virtual pointer, prints `ready`, and carries out
 * the commands of its standard input, one a line:
 *
 *   key down CODE      the key with that evdev code (1 to 767) goes down
 *   key up CODE        the key comes up
 *   button down CODE   the button with that evdev code (272 to 279) goes down
 *   button up CODE     the button comes up
 *   scroll DX DY       the wheel turns DX notches to the right and DY up
 *   sync               nothing
 *
 * A number of notches is an optional `-`, digits, and optionally a point
 * and one to eight digits, from -1000 to 1000, and a multiple of 1/2560.
 * Whole notches are sent as the steps of a wheel that clicks, 15 to a
 * step; a fraction as a smooth turn of 10 a notch. With a key it sends
 * the modifiers the keymap gives for the keys held, as a compositor does
 * for a real keyboard.
 *
 * Each command is answered with `ok` once the compositor has handled it.
 * A command it does not know or a number out of range stops it with
 * status 1. At the end of its input it removes its devices and exits
 * with status 0.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>
#include <wayland-client.h>
#include <xkbcommon/xkbcommon.h>

#include "virtual-keyboard-unstable-v1-client-protocol.h"
#include "wlr-virtual-pointer-unstable-v1-client-protocol.h"

#define MOST_OUTPUTS 16

static _Noreturn void fail(const char *format, ...)
    __attribute__((format(printf, 1, 2)));
static _Noreturn void fail(const char *format, ...) {
    va_list args;
    fputs("input: ", stderr);
    va_start(args, format);
    vfprintf(stderr, format, args);
    va_end(args);
    fputc('\n', stderr);
    exit(1);
}

/* An output and what it has announced. */
struct output {
    struct wl_output *output;
    char name[64];
    bool named;
    bool done;
};

static struct output outputs[MOST_OUTPUTS];
static int output_count;
/* The outputs have been checked: an output announced now stops the
 * client. */
static bool checked;

static struct wl_seat *seat;
static struct zwp_virtual_keyboard_manager_v1 *keyboards;
static struct zwlr_virtual_pointer_manager_v1 *pointers;

static void output_geometry(void *data, struct wl_output *output, int32_t x,
                            int32_t y, int32_t width, int32_t height,
                            int32_t subpixel, const char *make,
                            const char *model, int32_t transform) {
    (void)data, (void)output, (void)x, (void)y, (void)width, (void)height;
    (void)subpixel, (void)make, (void)model, (void)transform;
}

static void output_mode(void *data, struct wl_output *output, uint32_t flags,
                        int32_t width, int32_t height, int32_t refresh) {
    (void)data, (void)output, (void)flags, (void)width, (void)height,
        (void)refresh;
}

static void output_done(void *data, struct wl_output *output) {
    (void)output;
    struct output *out = data;
    if (!out->named) fail("an output announced no name; nothing was sent");
    out->done = true;
}

static void output_scale(void *data, struct wl_output *output, int32_t factor) {
    (void)data, (void)output, (void)factor;
}

static void output_name(void *data, struct wl_output *output, const char *name) {
    (void)output;
    struct output *out = data;
    snprintf(out->name, sizeof out->name, "%s", name);
    out->named = true;
}

static void output_description(void *data, struct wl_output *output,
                               const char *description) {
    (void)data, (void)output, (void)description;
}

static const struct wl_output_listener output_listener = {
    output_geometry, output_mode,        output_done,
    output_scale,    output_name,        output_description,
};

static void global(void *data, struct wl_registry *registry, uint32_t name,
                   const char *interface, uint32_t version) {
    (void)data;
    if (strcmp(interface, wl_output_interface.name) == 0) {
        if (checked) fail("an output was announced later; input stops here");
        if (version < 4) {
            fail("the compositor offers wl_output version %u, which names no "
                 "output; nothing was sent",
                 version);
        }
        if (output_count == MOST_OUTPUTS) fail("more than %d outputs", MOST_OUTPUTS);
        struct output *out = &outputs[output_count++];
        out->output = wl_registry_bind(registry, name, &wl_output_interface, 4);
        wl_output_add_listener(out->output, &output_listener, out);
    } else if (strcmp(interface, wl_seat_interface.name) == 0 && !seat) {
        seat = wl_registry_bind(registry, name, &wl_seat_interface, 1);
    } else if (strcmp(interface, zwp_virtual_keyboard_manager_v1_interface.name) == 0) {
        keyboards = wl_registry_bind(registry, name,
                                     &zwp_virtual_keyboard_manager_v1_interface, 1);
    } else if (strcmp(interface, zwlr_virtual_pointer_manager_v1_interface.name) == 0) {
        pointers = wl_registry_bind(registry, name,
                                    &zwlr_virtual_pointer_manager_v1_interface, 1);
    }
}

static void global_remove(void *data, struct wl_registry *registry, uint32_t name) {
    (void)data, (void)registry, (void)name;
}

static const struct wl_registry_listener registry_listener = {global, global_remove};

/* Whether `name` is `HEADLESS-` followed by one or more digits. */
static bool headless(const char *name) {
    const char *prefix = "HEADLESS-";
    if (strncmp(name, prefix, strlen(prefix)) != 0) return false;
    const char *digits = name + strlen(prefix);
    if (*digits == '\0') return false;
    for (const char *c = digits; *c != '\0'; c++) {
        if (*c < '0' || *c > '9') return false;
    }
    return true;
}

/* Connects to the socket at `path`, which must be absolute. */
static struct wl_display *connect_to(const char *path) {
    if (path[0] != '/') fail("the socket must be an absolute path: %s", path);
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    if (strlen(path) >= sizeof address.sun_path) fail("the socket's path is too long");
    strcpy(address.sun_path, path);
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0) fail("socket: %s", strerror(errno));
    if (connect(fd, (struct sockaddr *)&address, sizeof address) != 0) {
        fail("can't connect to %s: %s", path, strerror(errno));
    }
    struct wl_display *display = wl_display_connect_to_fd(fd);
    if (!display) fail("can't connect to %s", path);
    return display;
}

/* Waits until the compositor has handled every request sent. */
static void roundtrip(struct wl_display *display) {
    if (wl_display_roundtrip(display) < 0) fail("the compositor closed the connection");
}

static struct zwp_virtual_keyboard_v1 *keyboard;
static struct zwlr_virtual_pointer_v1 *pointer;
static struct xkb_state *state;
static uint32_t mods_sent[4];

static uint32_t now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint32_t)((uint64_t)ts.tv_sec * 1000 + (uint64_t)ts.tv_nsec / 1000000);
}

/* Makes the keyboard, with the US layout of xkb's `evdev` rules, and the
 * pointer. */
static void devices_make(struct wl_display *display) {
    struct xkb_context *context = xkb_context_new(XKB_CONTEXT_NO_ENVIRONMENT_NAMES);
    if (!context) fail("can't start xkbcommon");
    struct xkb_rule_names names = {"evdev", "pc105", "us", "", ""};
    struct xkb_keymap *keymap =
        xkb_keymap_new_from_names(context, &names, XKB_KEYMAP_COMPILE_NO_FLAGS);
    if (!keymap) fail("can't make the US keymap");
    state = xkb_state_new(keymap);
    char *text = xkb_keymap_get_as_string(keymap, XKB_KEYMAP_FORMAT_TEXT_V1);
    if (!state || !text) fail("can't make the US keymap");
    size_t size = strlen(text) + 1;
    int fd = memfd_create("input-keymap", MFD_CLOEXEC);
    if (fd < 0) fail("memfd_create: %s", strerror(errno));
    if (write(fd, text, size) != (ssize_t)size) fail("can't write the keymap");
    free(text);
    keyboard = zwp_virtual_keyboard_manager_v1_create_virtual_keyboard(keyboards, seat);
    zwp_virtual_keyboard_v1_keymap(keyboard, WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1, fd,
                                   (uint32_t)size);
    close(fd);
    xkb_keymap_unref(keymap);
    xkb_context_unref(context);
    pointer = zwlr_virtual_pointer_manager_v1_create_virtual_pointer(pointers, seat);
    roundtrip(display);
}

/* Sends a key, and the modifiers when the keys held change them. */
static void key(uint32_t code, bool down) {
    zwp_virtual_keyboard_v1_key(keyboard, now_ms(), code,
                                down ? WL_KEYBOARD_KEY_STATE_PRESSED
                                     : WL_KEYBOARD_KEY_STATE_RELEASED);
    xkb_state_update_key(state, code + 8, down ? XKB_KEY_DOWN : XKB_KEY_UP);
    uint32_t mods[4] = {
        xkb_state_serialize_mods(state, XKB_STATE_MODS_DEPRESSED),
        xkb_state_serialize_mods(state, XKB_STATE_MODS_LATCHED),
        xkb_state_serialize_mods(state, XKB_STATE_MODS_LOCKED),
        xkb_state_serialize_layout(state, XKB_STATE_LAYOUT_EFFECTIVE),
    };
    if (memcmp(mods, mods_sent, sizeof mods) != 0) {
        zwp_virtual_keyboard_v1_modifiers(keyboard, mods[0], mods[1], mods[2], mods[3]);
        memcpy(mods_sent, mods, sizeof mods);
    }
}

static void button(uint32_t code, bool down) {
    zwlr_virtual_pointer_v1_button(pointer, now_ms(), code,
                                   down ? WL_POINTER_BUTTON_STATE_PRESSED
                                        : WL_POINTER_BUTTON_STATE_RELEASED);
    zwlr_virtual_pointer_v1_frame(pointer);
}

/* A number of notches in 2560ths, from the text of a command; or false. */
static bool notches(const char *word, int64_t *out) {
    const char *c = word;
    bool negative = *c == '-';
    if (negative) c++;
    int64_t whole = 0;
    int digits = 0;
    for (; *c >= '0' && *c <= '9'; c++) {
        if (++digits > 4) return false;
        whole = whole * 10 + (*c - '0');
    }
    if (digits == 0) return false;
    int64_t fraction = 0;
    int64_t scale = 1;
    if (*c == '.') {
        c++;
        int places = 0;
        for (; *c >= '0' && *c <= '9'; c++) {
            if (++places > 8) return false;
            fraction = fraction * 10 + (*c - '0');
            scale *= 10;
        }
        if (places == 0) return false;
    }
    if (*c != '\0') return false;
    if (whole > 1000 || (whole == 1000 && fraction != 0)) return false;
    if (fraction * 2560 % scale != 0) return false;
    int64_t value = whole * 2560 + fraction * 2560 / scale;
    *out = negative ? -value : value;
    return true;
}

/* Turns the wheel by `value` 2560ths of a notch on `axis`, where Wayland's
 * positive is right or down. */
static void wheel_axis(enum wl_pointer_axis axis, int64_t value) {
    if (value == 0) return;
    if (value % 2560 == 0) {
        int32_t steps = (int32_t)(value / 2560);
        zwlr_virtual_pointer_v1_axis_discrete(pointer, now_ms(), axis,
                                              wl_fixed_from_int(15 * steps), steps);
    } else {
        /* 10 a notch, in wl_fixed's 256ths: value * 10 * 256 / 2560. */
        zwlr_virtual_pointer_v1_axis(pointer, now_ms(), axis, (wl_fixed_t)value);
    }
}

static void scroll(int64_t right, int64_t up) {
    if (right == 0 && up == 0) return;
    zwlr_virtual_pointer_v1_axis_source(pointer, WL_POINTER_AXIS_SOURCE_WHEEL);
    wheel_axis(WL_POINTER_AXIS_HORIZONTAL_SCROLL, right);
    wheel_axis(WL_POINTER_AXIS_VERTICAL_SCROLL, -up);
    zwlr_virtual_pointer_v1_frame(pointer);
}

/* A whole number from `least` to `most`, the whole of `word`; or -1. */
static int64_t code_of(const char *word, int64_t least, int64_t most) {
    int64_t value = 0;
    if (*word == '\0') return -1;
    for (const char *c = word; *c != '\0'; c++) {
        if (*c < '0' || *c > '9') return -1;
        value = value * 10 + (*c - '0');
        if (value > most) return -1;
    }
    return value < least ? -1 : value;
}

/* Carries out one command, given without its line break. */
static void command(char *line) {
    char *words[4];
    int count = 0;
    for (char *at = strtok(line, " \t"); at; at = strtok(NULL, " \t")) {
        if (count == 4) fail("too many words in a command");
        words[count++] = at;
    }
    if (count == 1 && strcmp(words[0], "sync") == 0) return;
    if (count == 3 && (strcmp(words[0], "key") == 0 || strcmp(words[0], "button") == 0)) {
        bool down = strcmp(words[1], "down") == 0;
        if (!down && strcmp(words[1], "up") != 0) {
            fail("`%s` takes `down` or `up`, not `%s`", words[0], words[1]);
        }
        if (words[0][0] == 'k') {
            int64_t code = code_of(words[2], 1, 767);
            if (code < 0) fail("a key's code is from 1 to 767, not `%s`", words[2]);
            key((uint32_t)code, down);
        } else {
            int64_t code = code_of(words[2], 272, 279);
            if (code < 0) fail("a button's code is from 272 to 279, not `%s`", words[2]);
            button((uint32_t)code, down);
        }
        return;
    }
    if (count == 3 && strcmp(words[0], "scroll") == 0) {
        int64_t right, up;
        if (!notches(words[1], &right) || !notches(words[2], &up)) {
            fail("`scroll` takes two numbers of notches from -1000 to 1000, "
                 "each a multiple of 1/2560");
        }
        scroll(right, up);
        return;
    }
    fail("unknown command `%s`", count == 0 ? "" : words[0]);
}

int main(int argc, char **argv) {
    if (argc != 2) fail("usage: input /ABSOLUTE/PATH/OF/THE/SOCKET");
    setvbuf(stdout, NULL, _IOLBF, 0);
    struct wl_display *display = connect_to(argv[1]);
    struct wl_registry *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &registry_listener, NULL);
    roundtrip(display);
    roundtrip(display);
    if (output_count == 0) fail("the compositor has no output; nothing was sent");
    for (int i = 0; i < output_count; i++) {
        if (!outputs[i].done) fail("an output announced no name; nothing was sent");
        if (!headless(outputs[i].name)) {
            fail("output %s is not headless; nothing was sent", outputs[i].name);
        }
    }
    checked = true;
    if (!seat || !keyboards || !pointers) {
        fail("the compositor has no seat, virtual keyboards or virtual pointers");
    }
    devices_make(display);
    printf("ready\n");

    char line[256];
    while (fgets(line, sizeof line, stdin)) {
        size_t len = strlen(line);
        if (len > 0 && line[len - 1] == '\n') {
            line[--len] = '\0';
        } else if (!feof(stdin)) {
            fail("a command is longer than %zu bytes", sizeof line - 2);
        }
        command(line);
        roundtrip(display);
        printf("ok\n");
    }
    zwp_virtual_keyboard_v1_destroy(keyboard);
    zwlr_virtual_pointer_v1_destroy(pointer);
    roundtrip(display);
    wl_display_disconnect(display);
    return 0;
}
