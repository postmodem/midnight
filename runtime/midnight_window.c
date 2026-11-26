/*
 * The window and its frames. See midnight_window.h.
 */

#include "midnight_window.h"
#include "midnight_gpu.h"

#include <math.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>

#include <webgpu/webgpu.h>
#include <webgpu/wgpu.h>

static struct {
    /* The frame texture and the frame it holds, when `holds`. */
    WGPUTexture texture;
    WGPUTextureView view;
    uint32_t width;
    uint32_t height;
    MnFrame drawn;
    bool holds;
    /* Where the frame is copied to be read back, and its size in bytes. */
    WGPUBuffer readback;
    uint64_t readback_size;
} win;

/* ---- Failures ------------------------------------------------------------ */

static _Noreturn void window_fail(const char *format, ...)
    __attribute__((format(printf, 1, 2)));

static _Noreturn void window_fail(const char *format, ...) {
    fflush(stdout);
    fputs("midnight: window error: ", stderr);
    va_list args;
    va_start(args, format);
    vfprintf(stderr, format, args);
    va_end(args);
    fputc('\n', stderr);
    mn_leave(70);
}

/* ---- The frame ----------------------------------------------------------- */

/* The linear value whose sRGB encoding is the byte. Cleared into an sRGB
 * texture, it leaves exactly that byte. */
static double linear_of(uint8_t byte) {
    double c = byte / 255.0;
    return c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4);
}

static void submit(WGPUCommandEncoder encoder) {
    WGPUCommandBuffer commands = wgpuCommandEncoderFinish(encoder, NULL);
    wgpuQueueSubmit(mn_gpu_queue(), 1, &commands);
    mn_gpu_submissions++;
    wgpuCommandBufferRelease(commands);
    wgpuCommandEncoderRelease(encoder);
}

static void frame_texture_drop(void) {
    if (win.view) wgpuTextureViewRelease(win.view);
    if (win.texture) {
        wgpuTextureDestroy(win.texture);
        wgpuTextureRelease(win.texture);
    }
    win.view = NULL;
    win.texture = NULL;
    win.width = 0;
    win.height = 0;
    win.holds = false;
}

/* Makes the frame texture `width` by `height` pixels, unless it is. */
static void frame_texture_size(uint32_t width, uint32_t height) {
    if (win.texture && win.width == width && win.height == height) return;
    frame_texture_drop();
    WGPUTextureDescriptor descriptor = {0};
    descriptor.usage = WGPUTextureUsage_RenderAttachment |
                       WGPUTextureUsage_TextureBinding |
                       WGPUTextureUsage_CopySrc;
    descriptor.dimension = WGPUTextureDimension_2D;
    descriptor.size = (WGPUExtent3D){width, height, 1};
    descriptor.format = WGPUTextureFormat_RGBA8UnormSrgb;
    descriptor.mipLevelCount = 1;
    descriptor.sampleCount = 1;
    win.texture = wgpuDeviceCreateTexture(mn_gpu_device(), &descriptor);
    if (!win.texture) {
        window_fail("can't make a frame of %ux%u pixels", width, height);
    }
    win.view = wgpuTextureCreateView(win.texture, NULL);
    win.width = width;
    win.height = height;
}

static bool frame_same(const MnFrame *a, const MnFrame *b) {
    return a->width == b->width && a->height == b->height &&
           a->background.r == b->background.r &&
           a->background.g == b->background.g &&
           a->background.b == b->background.b;
}

/* Records the drawing of `frame` into the frame texture: a clear to the
 * background, opaque. */
static void frame_record(WGPUCommandEncoder encoder, const MnFrame *frame) {
    frame_texture_size(frame->width, frame->height);
    WGPURenderPassColorAttachment attachment = {0};
    attachment.view = win.view;
    attachment.depthSlice = WGPU_DEPTH_SLICE_UNDEFINED;
    attachment.loadOp = WGPULoadOp_Clear;
    attachment.storeOp = WGPUStoreOp_Store;
    attachment.clearValue =
        (WGPUColor){linear_of(frame->background.r), linear_of(frame->background.g),
                    linear_of(frame->background.b), 1.0};
    WGPURenderPassDescriptor pass_descriptor = {0};
    pass_descriptor.colorAttachmentCount = 1;
    pass_descriptor.colorAttachments = &attachment;
    WGPURenderPassEncoder pass =
        wgpuCommandEncoderBeginRenderPass(encoder, &pass_descriptor);
    wgpuRenderPassEncoderEnd(pass);
    wgpuRenderPassEncoderRelease(pass);
    win.drawn = *frame;
    win.holds = true;
}

void mn_window_draw(const MnFrame *frame) {
    WGPUCommandEncoder encoder =
        wgpuDeviceCreateCommandEncoder(mn_gpu_device(), NULL);
    frame_record(encoder, frame);
    submit(encoder);
}

/* ---- Reading a frame back ------------------------------------------------ */

static void on_map(WGPUMapAsyncStatus status, WGPUStringView message,
                   void *done, void *unused) {
    (void)message;
    (void)unused;
    *(int *)done = status == WGPUMapAsyncStatus_Success ? 1 : -1;
}

/* Bytes per row of a copy to a buffer: a multiple of 256. */
static uint32_t padded_row(uint32_t width) { return (width * 4 + 255) & ~255u; }

const char *mn_window_read(const MnFrame *frame, unsigned char *rgba) {
    if (!mn_gpu_device() && !mn_window_device(NULL)) {
        return "the program has no device";
    }
    WGPUDevice device = mn_gpu_device();
    WGPUCommandEncoder encoder = wgpuDeviceCreateCommandEncoder(device, NULL);
    if (!win.holds || !frame_same(&win.drawn, frame)) {
        frame_record(encoder, frame);
    }
    uint32_t row = padded_row(frame->width);
    uint64_t size = (uint64_t)row * frame->height;
    if (win.readback_size < size) {
        if (win.readback) {
            wgpuBufferDestroy(win.readback);
            wgpuBufferRelease(win.readback);
        }
        WGPUBufferDescriptor descriptor = {0};
        descriptor.usage = WGPUBufferUsage_MapRead | WGPUBufferUsage_CopyDst;
        descriptor.size = size;
        win.readback = wgpuDeviceCreateBuffer(device, &descriptor);
        if (!win.readback) {
            window_fail("can't make a buffer of %llu bytes",
                        (unsigned long long)size);
        }
        win.readback_size = size;
    }
    WGPUTexelCopyTextureInfo from = {0};
    from.texture = win.texture;
    WGPUTexelCopyBufferInfo to = {0};
    to.buffer = win.readback;
    to.layout.bytesPerRow = row;
    to.layout.rowsPerImage = frame->height;
    WGPUExtent3D extent = {frame->width, frame->height, 1};
    wgpuCommandEncoderCopyTextureToBuffer(encoder, &from, &to, &extent);
    submit(encoder);

    int done = 0;
    WGPUBufferMapCallbackInfo callback = {0};
    callback.mode = WGPUCallbackMode_AllowProcessEvents;
    callback.callback = on_map;
    callback.userdata1 = &done;
    wgpuBufferMapAsync(win.readback, WGPUMapMode_Read, 0, (size_t)size, callback);
    mn_gpu_waits++;
    while (done == 0) wgpuDevicePoll(device, true, NULL);
    if (done < 0) window_fail("can't read a frame back");
    const unsigned char *mapped =
        wgpuBufferGetConstMappedRange(win.readback, 0, (size_t)size);
    for (uint32_t y = 0; y < frame->height; y++) {
        memcpy(rgba + (size_t)y * frame->width * 4, mapped + (size_t)y * row,
               (size_t)frame->width * 4);
    }
    wgpuBufferUnmap(win.readback);
    return NULL;
}

/* ---- The end ------------------------------------------------------------- */

void mn_window_finish(void) {
    frame_texture_drop();
    if (win.readback) {
        wgpuBufferDestroy(win.readback);
        wgpuBufferRelease(win.readback);
    }
    win.readback = NULL;
    win.readback_size = 0;
}
