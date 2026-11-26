/*
 * The window and its frames. See midnight_window.h.
 */

#include "midnight_window.h"
#include "midnight_gpu.h"

#include <math.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <SDL3/SDL.h>
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

    /* SDL's video part has been started. */
    bool sdl;
    /* The shown window, NULL when there is none; the surface on it and how
     * the surface is configured. */
    SDL_Window *window;
    WGPUSurface surface;
    WGPUSurfaceConfiguration config;
#ifdef __APPLE__
    SDL_MetalView metal;
#endif
    /* SDL says the window is hidden, minimized or occluded. */
    bool hidden;
    /* What draws the frame texture over the surface: made for the format
     * in `blit_format`, and a binding of the texture, remade with it. */
    WGPUTextureFormat blit_format;
    WGPUBindGroupLayout blit_layout;
    WGPURenderPipeline blit;
    WGPUBindGroup blit_group;
    /* What mn_window_show returns. */
    char how[64];
    char reason[300];
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

static WGPUStringView view(const char *text) {
    return (WGPUStringView){text, WGPU_STRLEN};
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
    if (win.blit_group) wgpuBindGroupRelease(win.blit_group);
    if (win.view) wgpuTextureViewRelease(win.view);
    if (win.texture) {
        wgpuTextureDestroy(win.texture);
        wgpuTextureRelease(win.texture);
    }
    win.blit_group = NULL;
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

/* ---- Drawing the frame over the surface ---------------------------------- */

/* A triangle that covers the target; each pixel is the frame's pixel at
 * the same place. */
static const char *BLIT_WGSL =
    "@group(0) @binding(0) var frame: texture_2d<f32>;\n"
    "@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {\n"
    "  let x = f32((i << 1u) & 2u);\n"
    "  let y = f32(i & 2u);\n"
    "  return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);\n"
    "}\n"
    "@fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {\n"
    "  return textureLoad(frame, vec2<i32>(p.xy), 0);\n"
    "}\n";

static void blit_drop(void) {
    if (win.blit_group) wgpuBindGroupRelease(win.blit_group);
    if (win.blit) wgpuRenderPipelineRelease(win.blit);
    if (win.blit_layout) wgpuBindGroupLayoutRelease(win.blit_layout);
    win.blit_group = NULL;
    win.blit = NULL;
    win.blit_layout = NULL;
    win.blit_format = WGPUTextureFormat_Undefined;
}

/* Makes the pipeline that draws to a surface of `format`, unless it is
 * made. */
static void blit_make(WGPUTextureFormat format) {
    if (win.blit && win.blit_format == format) return;
    blit_drop();
    WGPUDevice device = mn_gpu_device();
    WGPUShaderSourceWGSL source = {0};
    source.chain.sType = WGPUSType_ShaderSourceWGSL;
    source.code = view(BLIT_WGSL);
    WGPUShaderModuleDescriptor module_descriptor = {0};
    module_descriptor.nextInChain = &source.chain;
    WGPUShaderModule module = wgpuDeviceCreateShaderModule(device, &module_descriptor);

    WGPUBindGroupLayoutEntry entry = {0};
    entry.binding = 0;
    entry.visibility = WGPUShaderStage_Fragment;
    entry.texture.sampleType = WGPUTextureSampleType_Float;
    entry.texture.viewDimension = WGPUTextureViewDimension_2D;
    WGPUBindGroupLayoutDescriptor layout_descriptor = {0};
    layout_descriptor.entryCount = 1;
    layout_descriptor.entries = &entry;
    win.blit_layout = wgpuDeviceCreateBindGroupLayout(device, &layout_descriptor);
    WGPUPipelineLayoutDescriptor pipeline_layout_descriptor = {0};
    pipeline_layout_descriptor.bindGroupLayoutCount = 1;
    pipeline_layout_descriptor.bindGroupLayouts = &win.blit_layout;
    WGPUPipelineLayout pipeline_layout =
        wgpuDeviceCreatePipelineLayout(device, &pipeline_layout_descriptor);

    WGPUColorTargetState target = {0};
    target.format = format;
    target.writeMask = WGPUColorWriteMask_All;
    WGPUFragmentState fragment = {0};
    fragment.module = module;
    fragment.entryPoint = view("fs");
    fragment.targetCount = 1;
    fragment.targets = &target;
    WGPURenderPipelineDescriptor descriptor = {0};
    descriptor.layout = pipeline_layout;
    descriptor.vertex.module = module;
    descriptor.vertex.entryPoint = view("vs");
    descriptor.primitive.topology = WGPUPrimitiveTopology_TriangleList;
    descriptor.multisample.count = 1;
    descriptor.multisample.mask = ~0u;
    descriptor.fragment = &fragment;
    win.blit = wgpuDeviceCreateRenderPipeline(device, &descriptor);
    wgpuPipelineLayoutRelease(pipeline_layout);
    wgpuShaderModuleRelease(module);
    win.blit_format = format;
}

/* Records the pass that draws the frame texture over `target`. */
static void blit_record(WGPUCommandEncoder encoder, WGPUTextureView target) {
    if (!win.blit_group) {
        WGPUBindGroupEntry entry = {0};
        entry.binding = 0;
        entry.textureView = win.view;
        WGPUBindGroupDescriptor descriptor = {0};
        descriptor.layout = win.blit_layout;
        descriptor.entryCount = 1;
        descriptor.entries = &entry;
        win.blit_group = wgpuDeviceCreateBindGroup(mn_gpu_device(), &descriptor);
    }
    WGPURenderPassColorAttachment attachment = {0};
    attachment.view = target;
    attachment.depthSlice = WGPU_DEPTH_SLICE_UNDEFINED;
    attachment.loadOp = WGPULoadOp_Clear;
    attachment.storeOp = WGPUStoreOp_Store;
    attachment.clearValue = (WGPUColor){0.0, 0.0, 0.0, 1.0};
    WGPURenderPassDescriptor pass_descriptor = {0};
    pass_descriptor.colorAttachmentCount = 1;
    pass_descriptor.colorAttachments = &attachment;
    WGPURenderPassEncoder pass =
        wgpuCommandEncoderBeginRenderPass(encoder, &pass_descriptor);
    wgpuRenderPassEncoderSetPipeline(pass, win.blit);
    wgpuRenderPassEncoderSetBindGroup(pass, 0, win.blit_group, 0, NULL);
    wgpuRenderPassEncoderDraw(pass, 3, 1, 0, 0);
    wgpuRenderPassEncoderEnd(pass);
    wgpuRenderPassEncoderRelease(pass);
}

void mn_window_draw(const MnFrame *frame) {
    if (frame->width == 0 || frame->height == 0) {
        win.holds = false;
        return;
    }
    if (frame->width > MN_WINDOW_LARGEST || frame->height > MN_WINDOW_LARGEST) {
        window_fail("the desktop made the window %ux%u pixels, larger than %d by %d",
                    frame->width, frame->height, MN_WINDOW_LARGEST,
                    MN_WINDOW_LARGEST);
    }
    WGPUCommandEncoder encoder =
        wgpuDeviceCreateCommandEncoder(mn_gpu_device(), NULL);
    frame_record(encoder, frame);
    WGPUSurfaceTexture target = {0};
    if (win.window && !win.hidden && win.config.width == frame->width &&
        win.config.height == frame->height) {
        wgpuSurfaceGetCurrentTexture(win.surface, &target);
    }
    WGPUTextureView target_view = NULL;
    switch (target.status) {
    case WGPUSurfaceGetCurrentTextureStatus_SuccessOptimal:
    case WGPUSurfaceGetCurrentTextureStatus_SuccessSuboptimal:
        target_view = wgpuTextureCreateView(target.texture, NULL);
        blit_record(encoder, target_view);
        break;
    case WGPUSurfaceGetCurrentTextureStatus_Outdated:
    case WGPUSurfaceGetCurrentTextureStatus_Lost:
        wgpuSurfaceConfigure(win.surface, &win.config);
        break;
    case WGPUSurfaceGetCurrentTextureStatus_OutOfMemory:
    case WGPUSurfaceGetCurrentTextureStatus_DeviceLost:
    case WGPUSurfaceGetCurrentTextureStatus_Error:
        window_fail("the window's surface gave no texture (status %d)",
                    (int)target.status);
    default:
        break;
    }
    submit(encoder);
    if (target_view) {
        wgpuSurfacePresent(win.surface);
        wgpuTextureViewRelease(target_view);
    }
    if (target.texture) wgpuTextureRelease(target.texture);
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
    if (frame->width == 0 || frame->height == 0) return "the frame has no pixels";
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

/* ---- The shown window ---------------------------------------------------- */

/* Whether a display can be reached without guessing one: on Linux and the
 * BSDs a Wayland or X11 display must be named. */
static bool display_named(void) {
#ifdef __APPLE__
    return true;
#else
    const char *names[] = {"WAYLAND_DISPLAY", "WAYLAND_SOCKET", "DISPLAY"};
    for (size_t i = 0; i < sizeof names / sizeof names[0]; i++) {
        const char *value = getenv(names[i]);
        if (value && *value != '\0') return true;
    }
    return false;
#endif
}

/* A surface on the shown window from the native handles SDL gives for
 * `driver`, or NULL. */
static WGPUSurface surface_for(const char *driver) {
    SDL_PropertiesID props = SDL_GetWindowProperties(win.window);
    WGPUSurfaceDescriptor descriptor = {0};
    WGPUSurfaceSourceWaylandSurface wayland = {0};
    WGPUSurfaceSourceXlibWindow xlib = {0};
#ifdef __APPLE__
    WGPUSurfaceSourceMetalLayer metal = {0};
#endif
    if (strcmp(driver, "wayland") == 0) {
        wayland.chain.sType = WGPUSType_SurfaceSourceWaylandSurface;
        wayland.display = SDL_GetPointerProperty(
            props, SDL_PROP_WINDOW_WAYLAND_DISPLAY_POINTER, NULL);
        wayland.surface = SDL_GetPointerProperty(
            props, SDL_PROP_WINDOW_WAYLAND_SURFACE_POINTER, NULL);
        if (!wayland.display || !wayland.surface) return NULL;
        descriptor.nextInChain = &wayland.chain;
    } else if (strcmp(driver, "x11") == 0) {
        xlib.chain.sType = WGPUSType_SurfaceSourceXlibWindow;
        xlib.display = SDL_GetPointerProperty(
            props, SDL_PROP_WINDOW_X11_DISPLAY_POINTER, NULL);
        xlib.window = (uint64_t)SDL_GetNumberProperty(
            props, SDL_PROP_WINDOW_X11_WINDOW_NUMBER, 0);
        if (!xlib.display || !xlib.window) return NULL;
        descriptor.nextInChain = &xlib.chain;
#ifdef __APPLE__
    } else if (strcmp(driver, "cocoa") == 0) {
        win.metal = SDL_Metal_CreateView(win.window);
        if (!win.metal) return NULL;
        metal.chain.sType = WGPUSType_SurfaceSourceMetalLayer;
        metal.layer = SDL_Metal_GetLayer(win.metal);
        descriptor.nextInChain = &metal.chain;
#endif
    } else {
        return NULL;
    }
    return wgpuInstanceCreateSurface(mn_gpu_instance(), &descriptor);
}

static const char *mode_name(WGPUPresentMode mode) {
    switch (mode) {
    case WGPUPresentMode_Mailbox:
        return "mailbox";
    case WGPUPresentMode_Immediate:
        return "immediate";
    default:
        return "fifo";
    }
}

/* Builds the surface's configuration from what it lists for the device:
 * an 8-bit sRGB format by name, the first of mailbox, immediate and FIFO,
 * opaque when it can be. Returns false when it lists no such format or
 * mode, or can't be drawn to. */
static bool surface_config(void) {
    WGPUSurfaceCapabilities caps = {0};
    if (wgpuSurfaceGetCapabilities(win.surface, mn_gpu_adapter(), &caps) !=
        WGPUStatus_Success) {
        return false;
    }
    WGPUSurfaceConfiguration config = {0};
    config.device = mn_gpu_device();
    config.usage = WGPUTextureUsage_RenderAttachment;
    static const WGPUTextureFormat formats[] = {WGPUTextureFormat_RGBA8UnormSrgb,
                                                WGPUTextureFormat_BGRA8UnormSrgb};
    for (size_t f = 0; f < 2 && !config.format; f++) {
        for (size_t i = 0; i < caps.formatCount; i++) {
            if (caps.formats[i] == formats[f]) config.format = formats[f];
        }
    }
    static const WGPUPresentMode modes[] = {
        WGPUPresentMode_Mailbox, WGPUPresentMode_Immediate, WGPUPresentMode_Fifo};
    for (size_t m = 0; m < 3 && !config.presentMode; m++) {
        for (size_t i = 0; i < caps.presentModeCount; i++) {
            if (caps.presentModes[i] == modes[m]) config.presentMode = modes[m];
        }
    }
    config.alphaMode = WGPUCompositeAlphaMode_Auto;
    for (size_t i = 0; i < caps.alphaModeCount; i++) {
        if (caps.alphaModes[i] == WGPUCompositeAlphaMode_Opaque) {
            config.alphaMode = WGPUCompositeAlphaMode_Opaque;
        }
    }
    bool drawable = (caps.usages & WGPUTextureUsage_RenderAttachment) != 0;
    wgpuSurfaceCapabilitiesFreeMembers(caps);
    if (!config.format || !config.presentMode || !drawable) return false;
    win.config = config;
    return true;
}

/* The shown window's sizes now. */
static void sizes_now(MnWindowSizes *sizes) {
    int width = 0, height = 0, pixel_width = 0, pixel_height = 0;
    SDL_GetWindowSize(win.window, &width, &height);
    SDL_GetWindowSizeInPixels(win.window, &pixel_width, &pixel_height);
    sizes->width = width;
    sizes->height = height;
    sizes->pixel_width = pixel_width;
    sizes->pixel_height = pixel_height;
}

/* Configures the surface for the window's pixel size, when it has one and
 * it is not what the surface has. */
static void surface_size(const MnWindowSizes *sizes) {
    uint32_t width = (uint32_t)sizes->pixel_width;
    uint32_t height = (uint32_t)sizes->pixel_height;
    if (width == 0 || height == 0) return;
    if (width == win.config.width && height == win.config.height) return;
    win.config.width = width;
    win.config.height = height;
    wgpuSurfaceConfigure(win.surface, &win.config);
}

const char *mn_window_show(const char *title, int64_t width, int64_t height,
                           MnWindowSizes *sizes, const char **how) {
    if (!display_named()) return "no display was found";
    if (!win.sdl) {
#ifndef __APPLE__
        SDL_SetHint(SDL_HINT_VIDEO_DRIVER, "wayland,x11");
#endif
        SDL_SetHint(SDL_HINT_NO_SIGNAL_HANDLERS, "1");
        if (!SDL_InitSubSystem(SDL_INIT_VIDEO)) return "no display was found";
        win.sdl = true;
    }
    win.window = SDL_CreateWindow(title, (int)width, (int)height,
                                  SDL_WINDOW_RESIZABLE |
                                      SDL_WINDOW_HIGH_PIXEL_DENSITY);
    if (!win.window) {
        snprintf(win.reason, sizeof win.reason, "SDL can't make a window: %s",
                 SDL_GetError());
        return win.reason;
    }
    win.hidden = false;
    const char *driver = SDL_GetCurrentVideoDriver();
    if (!driver) driver = "";
    if (!mn_gpu_instance()) {
        mn_window_close();
        return "wgpu-native has no instance";
    }
    win.surface = surface_for(driver);
    if (!win.surface) {
        snprintf(win.reason, sizeof win.reason,
                 "no surface can be made on a window of SDL's `%s`", driver);
        mn_window_close();
        return win.reason;
    }
    if (!mn_gpu_device() && !mn_window_device(win.surface)) {
        mn_window_close();
        return "no device can draw to the desktop";
    }
    if (!surface_config()) {
        snprintf(win.reason, sizeof win.reason, "%s can't draw to the desktop",
                 mn_gpu_device_name());
        mn_window_close();
        return win.reason;
    }
    blit_make(win.config.format);
    sizes_now(sizes);
    sizes->closed = false;
    win.config.width = 0;
    win.config.height = 0;
    surface_size(sizes);
    snprintf(win.how, sizeof win.how, "%s, %s", driver,
             mode_name(win.config.presentMode));
    *how = win.how;
    return NULL;
}

void mn_window_poll(MnWindowSizes *sizes) {
    SDL_Event event;
    bool closed = false;
    while (SDL_PollEvent(&event)) {
        switch (event.type) {
        case SDL_EVENT_QUIT:
        case SDL_EVENT_WINDOW_CLOSE_REQUESTED:
            closed = true;
            break;
        case SDL_EVENT_WINDOW_HIDDEN:
        case SDL_EVENT_WINDOW_MINIMIZED:
        case SDL_EVENT_WINDOW_OCCLUDED:
            win.hidden = true;
            break;
        case SDL_EVENT_WINDOW_SHOWN:
        case SDL_EVENT_WINDOW_RESTORED:
        case SDL_EVENT_WINDOW_EXPOSED:
            win.hidden = false;
            break;
        default:
            break;
        }
    }
    sizes_now(sizes);
    sizes->closed = closed;
    surface_size(sizes);
}

void mn_window_close(void) {
    if (win.surface) {
        if (win.config.width != 0) wgpuSurfaceUnconfigure(win.surface);
        wgpuSurfaceRelease(win.surface);
    }
    win.surface = NULL;
    win.config = (WGPUSurfaceConfiguration){0};
#ifdef __APPLE__
    if (win.metal) SDL_Metal_DestroyView(win.metal);
    win.metal = NULL;
#endif
    if (win.window) SDL_DestroyWindow(win.window);
    win.window = NULL;
}

/* ---- The end ------------------------------------------------------------- */

void mn_window_finish(void) {
    mn_window_close();
    blit_drop();
    frame_texture_drop();
    if (win.readback) {
        wgpuBufferDestroy(win.readback);
        wgpuBufferRelease(win.readback);
    }
    win.readback = NULL;
    win.readback_size = 0;
    if (win.sdl) SDL_Quit();
    win.sdl = false;
}
