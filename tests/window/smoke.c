/* A window from SDL 3 that shows frames drawn with wgpu-native, as a
 * Midnight program with a window does:
 *
 *   smoke window CAPTURE.ppm
 *   smoke compare CAPTURE.ppm SCREEN.ppm
 *
 * `window` opens a window, makes a wgpu surface from its native handles
 * and a device that can draw to it, and shows a frame sixty times a
 * second. A frame is a texture the program owns, cleared to one colour
 * and drawn to the surface by a triangle that covers it. After 30 frames
 * the owned texture is read back and written to CAPTURE.ppm, and
 * `smoke: shown` is printed. The program ends with status 0 when the
 * window is closed from outside, and with status 1 when it is not closed
 * within 20 seconds or something fails.
 *
 * `compare` requires every pixel of the capture to be the colour the
 * frames are cleared to, and the pixel at the same place in SCREEN.ppm (a
 * picture of the compositor's output, with the window at its top left)
 * to be the same. */
#define _POSIX_C_SOURCE 200809L
#include <math.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include <SDL3/SDL.h>
#include <webgpu/webgpu.h>
#include <webgpu/wgpu.h>

/* The bytes every frame is cleared to: red, green, blue. */
static const unsigned char FRAME_COLOR[3] = {200, 100, 30};

#define CAPTURE_AT 30u
#define TICKS_PER_SECOND 60.0
#define SECONDS_TO_CLOSE 20.0

/* ---- Under AddressSanitizer ---------------------------------------------- */

#if defined(__has_feature)
#if __has_feature(address_sanitizer) && defined(__linux__)
#include <dlfcn.h>

/* Allocations made by GPU drivers, which the program cannot free. */
const char *__lsan_default_suppressions(void);
const char *__lsan_default_suppressions(void) {
    return "leak:<unknown module>\n";
}

const char *__lsan_default_options(void);
const char *__lsan_default_options(void) { return "print_suppressions=0"; }

extern void *__interceptor_dlopen(const char *file, int mode)
    __attribute__((weak));

/* dlopen in place of AddressSanitizer's, so that a library opened by name
 * is looked for in the runpath of the library that opens it, as
 * runtime/midnight_gpu.c does in a debug build. */
void *dlopen(const char *file, int mode) {
    static void *(*next)(const char *, int);
    if (!next) next = (void *(*)(const char *, int))dlsym(RTLD_NEXT, "dlopen");
    if ((mode & RTLD_DEEPBIND) && __interceptor_dlopen) {
        return __interceptor_dlopen(file, mode);
    }
    void *program = next(NULL, RTLD_LAZY);
    if (program) dlclose(program);
    __attribute__((musttail)) return next(file, mode);
}
#endif
#endif

/* ---- Small things -------------------------------------------------------- */

static _Noreturn void fail(const char *what, const char *detail) {
    fflush(stdout);
    fprintf(stderr, "smoke: %s%s%s\n", what, detail ? ": " : "", detail ? detail : "");
    exit(1);
}

static double now(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (double)t.tv_sec + (double)t.tv_nsec * 1e-9;
}

static void sleep_until(double when) {
    struct timespec t;
    t.tv_sec = (time_t)when;
    t.tv_nsec = (long)((when - (double)t.tv_sec) * 1e9);
    while (clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &t, NULL) != 0) {
    }
}

static WGPUStringView sv(const char *text) { return (WGPUStringView){text, WGPU_STRLEN}; }

static int sv_length(WGPUStringView text) {
    if (!text.data) return 0;
    return (int)(text.length == WGPU_STRLEN ? strlen(text.data) : text.length);
}

/* ---- PPM ------------------------------------------------------------------ */

/* Writes `rgba`, `width` by `height` pixels of 4 bytes, as a binary PPM. */
static void ppm_write(const char *path, uint32_t width, uint32_t height,
                      const unsigned char *rgba) {
    FILE *file = fopen(path, "wb");
    if (!file) fail("can't write", path);
    fprintf(file, "P6\n%u %u\n255\n", width, height);
    for (size_t i = 0; i < (size_t)width * height; i++) fwrite(rgba + i * 4, 1, 3, file);
    if (fclose(file) != 0) fail("can't write", path);
}

/* Reads a binary PPM of 8-bit samples. Returns its RGB bytes. */
static unsigned char *ppm_read(const char *path, uint32_t *width, uint32_t *height) {
    FILE *file = fopen(path, "rb");
    if (!file) fail("can't read", path);
    unsigned max = 0;
    if (fscanf(file, "P6 %u %u %u", width, height, &max) != 3 || max != 255 ||
        fgetc(file) == EOF) {
        fail("not a PPM of 8-bit samples", path);
    }
    size_t size = (size_t)*width * *height * 3;
    unsigned char *rgb = malloc(size ? size : 1);
    if (!rgb || fread(rgb, 1, size, file) != size) fail("short PPM", path);
    fclose(file);
    return rgb;
}

static int compare(const char *capture_path, const char *screen_path) {
    uint32_t cw, ch, sw, sh;
    unsigned char *capture = ppm_read(capture_path, &cw, &ch);
    unsigned char *screen = ppm_read(screen_path, &sw, &sh);
    if (cw == 0 || ch == 0 || cw > sw || ch > sh) {
        fprintf(stderr, "smoke: a capture of %ux%u does not fit a screen of %ux%u\n", cw, ch,
                sw, sh);
        return 1;
    }
    size_t wrong_colour = 0, wrong_screen = 0;
    for (uint32_t y = 0; y < ch; y++) {
        for (uint32_t x = 0; x < cw; x++) {
            const unsigned char *c = capture + ((size_t)y * cw + x) * 3;
            const unsigned char *s = screen + ((size_t)y * sw + x) * 3;
            wrong_colour += memcmp(c, FRAME_COLOR, 3) != 0;
            wrong_screen += memcmp(c, s, 3) != 0;
        }
    }
    printf("smoke: capture %ux%u, screen %ux%u, pixel 0 of the screen %u,%u,%u: "
           "%zu pixels of the capture not %u,%u,%u, %zu not on the screen\n",
           cw, ch, sw, sh, screen[0], screen[1], screen[2], wrong_colour, FRAME_COLOR[0],
           FRAME_COLOR[1], FRAME_COLOR[2], wrong_screen);
    free(capture);
    free(screen);
    return wrong_colour != 0 || wrong_screen != 0;
}

/* ---- The device ----------------------------------------------------------- */

static WGPUInstance instance;
static WGPUAdapter adapter;
static WGPUDevice device;
static WGPUQueue queue;
static int device_errors;

static void on_adapter(WGPURequestAdapterStatus status, WGPUAdapter found,
                       WGPUStringView message, void *unused1, void *unused2) {
    (void)unused1;
    (void)unused2;
    if (status == WGPURequestAdapterStatus_Success) {
        adapter = found;
    } else {
        fprintf(stderr, "smoke: no adapter: %.*s\n", sv_length(message), message.data);
    }
}

static void on_device(WGPURequestDeviceStatus status, WGPUDevice found,
                      WGPUStringView message, void *unused1, void *unused2) {
    (void)unused1;
    (void)unused2;
    if (status == WGPURequestDeviceStatus_Success) {
        device = found;
    } else {
        fprintf(stderr, "smoke: no device: %.*s\n", sv_length(message), message.data);
    }
}

static void on_error(WGPUDevice const *unused, WGPUErrorType type, WGPUStringView message,
                     void *unused1, void *unused2) {
    (void)unused;
    (void)unused1;
    (void)unused2;
    device_errors++;
    fprintf(stderr, "smoke: device error %d: %.*s\n", (int)type, sv_length(message),
            message.data);
}

static void on_map(WGPUMapAsyncStatus status, WGPUStringView message, void *done,
                   void *unused) {
    (void)message;
    (void)unused;
    *(int *)done = status == WGPUMapAsyncStatus_Success ? 1 : -1;
}

static void instance_start(void) {
    WGPUInstanceExtras extras = {0};
    extras.chain.sType = (WGPUSType)WGPUSType_InstanceExtras;
    extras.backends = WGPUInstanceBackend_Primary;
    WGPUInstanceDescriptor descriptor = {0};
    descriptor.nextInChain = &extras.chain;
    instance = wgpuCreateInstance(&descriptor);
    if (!instance) fail("no wgpu instance", NULL);
}

/* The adapter and device: the fastest adapter that can draw to `surface`,
 * with the limits it offers. */
static void device_start(WGPUSurface surface) {
    WGPURequestAdapterOptions options = {0};
    options.featureLevel = WGPUFeatureLevel_Core;
    options.powerPreference = WGPUPowerPreference_HighPerformance;
    options.compatibleSurface = surface;
    WGPURequestAdapterCallbackInfo adapter_callback = {0};
    adapter_callback.mode = WGPUCallbackMode_AllowSpontaneous;
    adapter_callback.callback = on_adapter;
    wgpuInstanceRequestAdapter(instance, &options, adapter_callback);
    wgpuInstanceProcessEvents(instance);
    if (!adapter) fail("no adapter can draw to the window", NULL);

    WGPUAdapterInfo info = {0};
    wgpuAdapterGetInfo(adapter, &info);
    printf("smoke: adapter %.*s (%s)\n", sv_length(info.device), info.device.data,
           info.adapterType == WGPUAdapterType_CPU ? "cpu" : "gpu");
    wgpuAdapterInfoFreeMembers(info);

    WGPULimits limits = {0};
    wgpuAdapterGetLimits(adapter, &limits);
    WGPUDeviceDescriptor descriptor = {0};
    descriptor.requiredLimits = &limits;
    descriptor.uncapturedErrorCallbackInfo.callback = on_error;
    WGPURequestDeviceCallbackInfo device_callback = {0};
    device_callback.mode = WGPUCallbackMode_AllowSpontaneous;
    device_callback.callback = on_device;
    wgpuAdapterRequestDevice(adapter, &descriptor, device_callback);
    wgpuInstanceProcessEvents(instance);
    if (!device) fail("no device", NULL);
    queue = wgpuDeviceGetQueue(device);
}

static void device_finish(void) {
    wgpuQueueRelease(queue);
    wgpuDeviceRelease(device);
    wgpuAdapterRelease(adapter);
    wgpuInstanceRelease(instance);
}

static void submit(WGPUCommandEncoder encoder) {
    WGPUCommandBuffer commands = wgpuCommandEncoderFinish(encoder, NULL);
    wgpuQueueSubmit(queue, 1, &commands);
    wgpuCommandBufferRelease(commands);
    wgpuCommandEncoderRelease(encoder);
}

/* ---- The surface ---------------------------------------------------------- */

/* A surface on the window, from the native handles SDL gives. */
static WGPUSurface surface_for(SDL_Window *window) {
    SDL_PropertiesID props = SDL_GetWindowProperties(window);
    const char *driver = SDL_GetCurrentVideoDriver();
    WGPUSurfaceDescriptor descriptor = {0};
    WGPUSurfaceSourceWaylandSurface wayland = {0};
    WGPUSurfaceSourceXlibWindow xlib = {0};
    if (strcmp(driver, "wayland") == 0) {
        wayland.chain.sType = WGPUSType_SurfaceSourceWaylandSurface;
        wayland.display =
            SDL_GetPointerProperty(props, SDL_PROP_WINDOW_WAYLAND_DISPLAY_POINTER, NULL);
        wayland.surface =
            SDL_GetPointerProperty(props, SDL_PROP_WINDOW_WAYLAND_SURFACE_POINTER, NULL);
        if (!wayland.display || !wayland.surface) fail("SDL gave no Wayland surface", NULL);
        descriptor.nextInChain = &wayland.chain;
    } else if (strcmp(driver, "x11") == 0) {
        xlib.chain.sType = WGPUSType_SurfaceSourceXlibWindow;
        xlib.display = SDL_GetPointerProperty(props, SDL_PROP_WINDOW_X11_DISPLAY_POINTER, NULL);
        xlib.window = (uint64_t)SDL_GetNumberProperty(props, SDL_PROP_WINDOW_X11_WINDOW_NUMBER, 0);
        if (!xlib.display || !xlib.window) fail("SDL gave no X11 window", NULL);
        descriptor.nextInChain = &xlib.chain;
    } else {
        fail("no surface for the SDL video driver", driver);
    }
    WGPUSurface surface = wgpuInstanceCreateSurface(instance, &descriptor);
    if (!surface) fail("no surface", NULL);
    return surface;
}

static const char *mode_name(WGPUPresentMode mode) {
    switch (mode) {
    case WGPUPresentMode_Mailbox: return "mailbox";
    case WGPUPresentMode_Immediate: return "immediate";
    case WGPUPresentMode_Fifo: return "fifo";
    default: return "other";
    }
}

/* A configuration of the surface built from what it lists: an 8-bit sRGB
 * format, the first of mailbox, immediate and FIFO, and opaque. The
 * caller sets the size. */
static WGPUSurfaceConfiguration surface_config(WGPUSurface surface) {
    WGPUSurfaceCapabilities caps = {0};
    wgpuSurfaceGetCapabilities(surface, adapter, &caps);
    WGPUSurfaceConfiguration config = {0};
    config.device = device;
    config.usage = WGPUTextureUsage_RenderAttachment;
    static const WGPUTextureFormat formats[] = {WGPUTextureFormat_RGBA8UnormSrgb,
                                                WGPUTextureFormat_BGRA8UnormSrgb};
    for (size_t f = 0; f < 2 && !config.format; f++) {
        for (size_t i = 0; i < caps.formatCount; i++) {
            if (caps.formats[i] == formats[f]) config.format = formats[f];
        }
    }
    static const WGPUPresentMode modes[] = {WGPUPresentMode_Mailbox, WGPUPresentMode_Immediate,
                                            WGPUPresentMode_Fifo};
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
    bool usable = (caps.usages & WGPUTextureUsage_RenderAttachment) != 0;
    wgpuSurfaceCapabilitiesFreeMembers(caps);
    if (!config.format) fail("the window's surface lists no 8-bit sRGB format", NULL);
    if (!config.presentMode) fail("the window's surface lists no present mode", NULL);
    if (!usable) fail("the window's surface can't be drawn to", NULL);
    return config;
}

/* ---- The frame ------------------------------------------------------------ */

/* The frame: a texture the program owns, of the window's pixel size, and
 * what draws it to the surface. */
typedef struct Frame {
    uint32_t width, height;
    WGPUTexture texture;
    WGPUTextureView view;
    WGPUBindGroup group;
} Frame;

/* What draws a frame to the surface, made once for the surface's format. */
typedef struct Blit {
    WGPUBindGroupLayout layout;
    WGPURenderPipeline pipeline;
} Blit;

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

static Blit blit_make(WGPUTextureFormat format) {
    Blit blit = {0};
    WGPUShaderSourceWGSL source = {0};
    source.chain.sType = WGPUSType_ShaderSourceWGSL;
    source.code = sv(BLIT_WGSL);
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
    blit.layout = wgpuDeviceCreateBindGroupLayout(device, &layout_descriptor);
    WGPUPipelineLayoutDescriptor pipeline_layout_descriptor = {0};
    pipeline_layout_descriptor.bindGroupLayoutCount = 1;
    pipeline_layout_descriptor.bindGroupLayouts = &blit.layout;
    WGPUPipelineLayout pipeline_layout =
        wgpuDeviceCreatePipelineLayout(device, &pipeline_layout_descriptor);

    WGPUColorTargetState target = {0};
    target.format = format;
    target.writeMask = WGPUColorWriteMask_All;
    WGPUFragmentState fragment = {0};
    fragment.module = module;
    fragment.entryPoint = sv("fs");
    fragment.targetCount = 1;
    fragment.targets = &target;
    WGPURenderPipelineDescriptor descriptor = {0};
    descriptor.layout = pipeline_layout;
    descriptor.vertex.module = module;
    descriptor.vertex.entryPoint = sv("vs");
    descriptor.primitive.topology = WGPUPrimitiveTopology_TriangleList;
    descriptor.multisample.count = 1;
    descriptor.multisample.mask = ~0u;
    descriptor.fragment = &fragment;
    blit.pipeline = wgpuDeviceCreateRenderPipeline(device, &descriptor);
    wgpuPipelineLayoutRelease(pipeline_layout);
    wgpuShaderModuleRelease(module);
    return blit;
}

static void blit_drop(Blit *blit) {
    wgpuRenderPipelineRelease(blit->pipeline);
    wgpuBindGroupLayoutRelease(blit->layout);
}

static Frame frame_make(const Blit *blit, uint32_t width, uint32_t height) {
    Frame frame = {width, height, NULL, NULL, NULL};
    WGPUTextureDescriptor descriptor = {0};
    descriptor.usage = WGPUTextureUsage_RenderAttachment | WGPUTextureUsage_TextureBinding |
                       WGPUTextureUsage_CopySrc;
    descriptor.dimension = WGPUTextureDimension_2D;
    descriptor.size = (WGPUExtent3D){width, height, 1};
    descriptor.format = WGPUTextureFormat_RGBA8UnormSrgb;
    descriptor.mipLevelCount = 1;
    descriptor.sampleCount = 1;
    frame.texture = wgpuDeviceCreateTexture(device, &descriptor);
    frame.view = wgpuTextureCreateView(frame.texture, NULL);
    WGPUBindGroupEntry entry = {0};
    entry.binding = 0;
    entry.textureView = frame.view;
    WGPUBindGroupDescriptor group_descriptor = {0};
    group_descriptor.layout = blit->layout;
    group_descriptor.entryCount = 1;
    group_descriptor.entries = &entry;
    frame.group = wgpuDeviceCreateBindGroup(device, &group_descriptor);
    return frame;
}

static void frame_drop(Frame *frame) {
    wgpuBindGroupRelease(frame->group);
    wgpuTextureViewRelease(frame->view);
    wgpuTextureRelease(frame->texture);
    *frame = (Frame){0};
}

/* The linear value whose sRGB encoding is the byte. */
static double linear_of(unsigned char byte) {
    double c = byte / 255.0;
    return c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4);
}

/* Records a clear of the frame to the bytes `rgb`, opaque. */
static void frame_clear(WGPUCommandEncoder encoder, const Frame *frame,
                        const unsigned char rgb[3]) {
    WGPURenderPassColorAttachment attachment = {0};
    attachment.view = frame->view;
    attachment.depthSlice = WGPU_DEPTH_SLICE_UNDEFINED;
    attachment.loadOp = WGPULoadOp_Clear;
    attachment.storeOp = WGPUStoreOp_Store;
    attachment.clearValue = (WGPUColor){linear_of(rgb[0]), linear_of(rgb[1]),
                                        linear_of(rgb[2]), 1.0};
    WGPURenderPassDescriptor pass_descriptor = {0};
    pass_descriptor.colorAttachmentCount = 1;
    pass_descriptor.colorAttachments = &attachment;
    WGPURenderPassEncoder pass = wgpuCommandEncoderBeginRenderPass(encoder, &pass_descriptor);
    wgpuRenderPassEncoderEnd(pass);
    wgpuRenderPassEncoderRelease(pass);
}

/* Records the pass that draws the frame over the whole of `target`. */
static void frame_blit(WGPUCommandEncoder encoder, const Blit *blit, const Frame *frame,
                       WGPUTextureView target) {
    WGPURenderPassColorAttachment attachment = {0};
    attachment.view = target;
    attachment.depthSlice = WGPU_DEPTH_SLICE_UNDEFINED;
    attachment.loadOp = WGPULoadOp_Clear;
    attachment.storeOp = WGPUStoreOp_Store;
    attachment.clearValue = (WGPUColor){0.0, 0.0, 0.0, 1.0};
    WGPURenderPassDescriptor pass_descriptor = {0};
    pass_descriptor.colorAttachmentCount = 1;
    pass_descriptor.colorAttachments = &attachment;
    WGPURenderPassEncoder pass = wgpuCommandEncoderBeginRenderPass(encoder, &pass_descriptor);
    wgpuRenderPassEncoderSetPipeline(pass, blit->pipeline);
    wgpuRenderPassEncoderSetBindGroup(pass, 0, frame->group, 0, NULL);
    wgpuRenderPassEncoderDraw(pass, 3, 1, 0, 0);
    wgpuRenderPassEncoderEnd(pass);
    wgpuRenderPassEncoderRelease(pass);
}

/* Bytes per row of a copy to a buffer: a multiple of 256. */
static uint32_t padded_row(uint32_t width) { return (width * 4 + 255) & ~255u; }

/* The frame's pixels, `width * height * 4` bytes of RGBA, rows top to
 * bottom. The caller frees them. */
static unsigned char *frame_read(const Frame *frame) {
    uint32_t row = padded_row(frame->width);
    uint64_t size = (uint64_t)row * frame->height;
    WGPUBufferDescriptor buffer_descriptor = {0};
    buffer_descriptor.usage = WGPUBufferUsage_MapRead | WGPUBufferUsage_CopyDst;
    buffer_descriptor.size = size;
    WGPUBuffer buffer = wgpuDeviceCreateBuffer(device, &buffer_descriptor);
    WGPUCommandEncoder encoder = wgpuDeviceCreateCommandEncoder(device, NULL);
    WGPUTexelCopyTextureInfo from = {0};
    from.texture = frame->texture;
    WGPUTexelCopyBufferInfo to = {0};
    to.buffer = buffer;
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
    wgpuBufferMapAsync(buffer, WGPUMapMode_Read, 0, size, callback);
    while (!done) wgpuDevicePoll(device, true, NULL);
    if (done < 0) fail("can't read the frame back", NULL);
    const unsigned char *mapped = wgpuBufferGetConstMappedRange(buffer, 0, size);
    unsigned char *rgba = malloc((size_t)frame->width * frame->height * 4);
    if (!rgba) fail("out of memory", NULL);
    for (uint32_t y = 0; y < frame->height; y++) {
        memcpy(rgba + (size_t)y * frame->width * 4, mapped + (size_t)y * row,
               (size_t)frame->width * 4);
    }
    wgpuBufferUnmap(buffer);
    wgpuBufferRelease(buffer);
    return rgba;
}

/* ---- The window ------------------------------------------------------------ */

static int window(const char *capture_path) {
    SDL_SetHint(SDL_HINT_VIDEO_DRIVER, "wayland,x11");
    SDL_SetHint(SDL_HINT_NO_SIGNAL_HANDLERS, "1");
    SDL_SetHint(SDL_HINT_SHUTDOWN_DBUS_ON_QUIT, "1");
    if (!SDL_Init(SDL_INIT_VIDEO)) fail("SDL_Init", SDL_GetError());
    SDL_Window *sdl_window = SDL_CreateWindow(
        "midnight smoke", 320, 200, SDL_WINDOW_RESIZABLE | SDL_WINDOW_HIGH_PIXEL_DENSITY);
    if (!sdl_window) fail("SDL_CreateWindow", SDL_GetError());

    instance_start();
    WGPUSurface surface = surface_for(sdl_window);
    device_start(surface);
    WGPUSurfaceConfiguration config = surface_config(surface);
    int width = 0, height = 0, pixel_width = 0, pixel_height = 0;
    SDL_GetWindowSize(sdl_window, &width, &height);
    SDL_GetWindowSizeInPixels(sdl_window, &pixel_width, &pixel_height);
    config.width = (uint32_t)pixel_width;
    config.height = (uint32_t)pixel_height;
    wgpuSurfaceConfigure(surface, &config);
    printf("smoke: %s, %dx%d points, %dx%d pixels, %s, %s\n", SDL_GetCurrentVideoDriver(),
           width, height, pixel_width, pixel_height,
           config.format == WGPUTextureFormat_RGBA8UnormSrgb ? "RGBA8UnormSrgb"
                                                             : "BGRA8UnormSrgb",
           mode_name(config.presentMode));
    Blit blit = blit_make(config.format);
    Frame frame = frame_make(&blit, config.width, config.height);

    bool hidden = false, closed = false;
    uint32_t frames = 0, shown = 0, skipped = 0, resizes = 0;
    double start = now(), due = start;
    while (!closed && now() - start < SECONDS_TO_CLOSE) {
        SDL_Event event;
        while (SDL_PollEvent(&event)) {
            switch (event.type) {
            case SDL_EVENT_QUIT:
            case SDL_EVENT_WINDOW_CLOSE_REQUESTED: closed = true; break;
            case SDL_EVENT_WINDOW_HIDDEN:
            case SDL_EVENT_WINDOW_MINIMIZED:
            case SDL_EVENT_WINDOW_OCCLUDED: hidden = true; break;
            case SDL_EVENT_WINDOW_SHOWN:
            case SDL_EVENT_WINDOW_RESTORED:
            case SDL_EVENT_WINDOW_EXPOSED: hidden = false; break;
            case SDL_EVENT_WINDOW_PIXEL_SIZE_CHANGED:
                config.width = (uint32_t)event.window.data1;
                config.height = (uint32_t)event.window.data2;
                wgpuSurfaceConfigure(surface, &config);
                frame_drop(&frame);
                frame = frame_make(&blit, config.width, config.height);
                resizes++;
                break;
            default: break;
            }
        }
        if (closed) break;

        WGPUCommandEncoder encoder = wgpuDeviceCreateCommandEncoder(device, NULL);
        frame_clear(encoder, &frame, FRAME_COLOR);
        WGPUSurfaceTexture target = {0};
        if (!hidden) wgpuSurfaceGetCurrentTexture(surface, &target);
        bool present = target.status == WGPUSurfaceGetCurrentTextureStatus_SuccessOptimal ||
                       target.status == WGPUSurfaceGetCurrentTextureStatus_SuccessSuboptimal;
        WGPUTextureView target_view = NULL;
        if (present) {
            target_view = wgpuTextureCreateView(target.texture, NULL);
            frame_blit(encoder, &blit, &frame, target_view);
        }
        submit(encoder);
        if (present) {
            wgpuSurfacePresent(surface);
            shown++;
        } else {
            skipped++;
        }
        if (target_view) wgpuTextureViewRelease(target_view);
        if (target.texture) wgpuTextureRelease(target.texture);
        if (target.status == WGPUSurfaceGetCurrentTextureStatus_Outdated ||
            target.status == WGPUSurfaceGetCurrentTextureStatus_Lost) {
            wgpuSurfaceConfigure(surface, &config);
        }
        frames++;

        if (frames == CAPTURE_AT) {
            unsigned char *rgba = frame_read(&frame);
            ppm_write(capture_path, frame.width, frame.height, rgba);
            free(rgba);
            printf("smoke: shown, capture %ux%u\n", frame.width, frame.height);
            fflush(stdout);
        }

        due += 1.0 / TICKS_PER_SECOND;
        if (due < now()) due = now();
        sleep_until(due);
    }
    double seconds = now() - start;
    printf("smoke: %u frames in %.2f s, %u presented, %u not, %u resizes, %d device errors, "
           "%s\n",
           frames, seconds, shown, skipped, resizes, device_errors,
           closed ? "closed" : "not closed");

    frame_drop(&frame);
    blit_drop(&blit);
    wgpuSurfaceUnconfigure(surface);
    wgpuSurfaceRelease(surface);
    device_finish();
    SDL_DestroyWindow(sdl_window);
    SDL_Quit();
    return closed && device_errors == 0 && frames >= CAPTURE_AT ? 0 : 1;
}

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IOLBF, 0);
    if (argc == 3 && strcmp(argv[1], "window") == 0) return window(argv[2]);
    if (argc == 4 && strcmp(argv[1], "compare") == 0) return compare(argv[2], argv[3]);
    fprintf(stderr, "usage: smoke window CAPTURE.ppm | smoke compare CAPTURE.ppm SCREEN.ppm\n");
    return 2;
}
