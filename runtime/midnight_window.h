/*
 * What midnight_rt.c and midnight_window.c need from each other.
 *
 * midnight_window.c is the part of the runtime that shows a window, with
 * SDL 3, and draws its frames on the device, with wgpu-native. It is
 * compiled, with midnight_gpu.c, only into programs with a window, and only
 * when both MIDNIGHT_GPU_FLAGS and MIDNIGHT_WINDOW_FLAGS are set. MN_FRAMES
 * and MN_GPU are then defined and midnight_rt.c includes this header.
 * Generated code never does.
 *
 * Everything here is called from the thread that runs the ticks.
 *
 * A frame is rendered into a texture midnight_window.c owns, RGBA8Unorm, of
 * the frame's size, holding colour bytes as they are. A window that is
 * shown gets the texture by one more pass that copies each texel, so a
 * frame read back is the same with a window and without one.
 */

#ifndef MIDNIGHT_WINDOW_H
#define MIDNIGHT_WINDOW_H

#include "midnight_rt.h"

#include "midnight_gpu.h"

/* Draw records of a frame: `count` records in CPU memory at `records`, or,
 * when `records` is NULL, the records a GPU block wrote on the device,
 * `on_device`. */
typedef struct MnSpan {
    const MnDraw *records;
    size_t count;
    MnGpuRecords on_device;
} MnSpan;

/* A frame: the window's size and background after a tick, and the draws of
 * the tick, span after span, each record in order. The background's alpha
 * is ignored. The spans stay valid until the next frame is made. */
typedef struct MnFrame {
    uint32_t width;  /* pixels, 1 to MN_WINDOW_LARGEST */
    uint32_t height; /* pixels, 1 to MN_WINDOW_LARGEST */
    int64_t point_width;
    int64_t point_height;
    MnColor background;
    const MnSpan *spans;
    size_t span_count;
    /* Goes up with every frame the runtime makes. */
    uint64_t number;
} MnFrame;

/* What the desktop says of a window that is shown. */
typedef struct MnWindowSizes {
    int64_t width;  /* points */
    int64_t height; /* points */
    int64_t pixel_width;
    int64_t pixel_height;
    /* The person at the desktop has asked for the window to be closed. */
    bool closed;
} MnWindowSizes;

/* ---- In midnight_rt.c ---------------------------------------------------- */

struct WGPUSurfaceImpl;

/* Starts the device, fitting `surface` when that is not NULL, unless it has
 * been asked for before or MIDNIGHT_GPU has `off`, and prints MIDNIGHT_GPU's
 * `info` line. Returns the device's name, or NULL when there is none. */
const char *mn_window_device(struct WGPUSurfaceImpl *surface);

/* What the desktop says of the keyboard, the mouse and the focus of the
 * shown window, in the order it happened; mn_window_poll calls them. Each
 * is something a command can say, and reaches the program when the next
 * tick begins. A driven window takes its input from its commands, and
 * these then drop what they are given.
 *
 * `key` is the number of a `Key` variant and `button` that of a
 * `MouseButton` variant; positions are in points from the window's top
 * left corner; the wheel is in notches, right and up positive. A move
 * after a leave is the pointer coming back. */
void mn_window_key(uint32_t key, bool down);
void mn_window_button(uint32_t button, bool down, double x, double y);
void mn_window_move(double x, double y);
void mn_window_leave(void);
void mn_window_wheel(double x, double y);
void mn_window_focus(bool gained);

/* ---- In midnight_window.c ------------------------------------------------ */

/* Shows a window of `width` by `height` points titled `title`, with SDL's
 * video driver for Wayland before the one for X11. Starts the device
 * through mn_window_device when there is none, asking for one that can draw
 * to the window. Returns NULL and the window's sizes in `sizes`, with
 * `how` set to where it is shown and how frames reach it ("wayland,
 * mailbox"); or why no window can be shown, with nothing left open but SDL
 * and the device. Neither string needs to be freed. Call it while no
 * window is shown. */
const char *mn_window_show(const char *title, int64_t width, int64_t height,
                           MnWindowSizes *sizes, const char **how);

/* Takes what has happened to the shown window since the last call: its
 * sizes now, and whether a close was asked for. A change of pixel size
 * configures the surface again. */
void mn_window_poll(MnWindowSizes *sizes);

/* While a window is shown and not hidden, minimized or occluded: waits for
 * the device to finish the work handed to it before, renders `frame` into
 * the frame texture and presents it. Renders nothing otherwise. Never
 * waits for the display. */
void mn_window_draw(const MnFrame *frame);

/* Writes the pixels of `frame` to `rgba`, width * height * 4 bytes, rows
 * from the top: reads the frame texture back, after rendering the frame
 * into it when it holds another. Starts the device through
 * mn_window_device when there is none. Returns NULL, or why the frame
 * can't be read. */
const char *mn_window_read(const MnFrame *frame, unsigned char *rgba);

/* Closes the shown window. */
void mn_window_close(void);

/* Releases everything midnight_window.c made, and shuts SDL down when it
 * was started. Call before mn_gpu_device_finish. */
void mn_window_finish(void);

#endif
