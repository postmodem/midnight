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
    /* The frame texture and the number of the frame it holds, when
     * `holds`. */
    WGPUTexture texture;
    WGPUTextureView view;
    uint32_t width;
    uint32_t height;
    uint64_t drawn;
    bool holds;
    /* What renders shapes: the pipeline, its uniform buffer and their
     * binding, and the buffer that records from CPU memory are sent to,
     * with how many it holds. */
    WGPURenderPipeline shapes;
    WGPUBuffer shape_uniform;
    WGPUBindGroup shape_group;
    WGPUBuffer records;
    uint64_t records_capacity;
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
    va_list args;
    va_start(args, format);
    mn_device_fail(true, format, args);
}

static WGPUStringView view(const char *text) {
    return (WGPUStringView){text, WGPU_STRLEN};
}

/* ---- The frame ----------------------------------------------------------- */

/* Renders the records of a frame. Each record is an instance of six
 * vertices: the smallest upright rectangle of pixels that holds its shape,
 * one pixel larger on every side and cut to the frame. The fragment stage
 * decides which pixels the shape covers, from the pixel's center in
 * points, by the tests of the reference's "Which pixels a shape covers".
 * Every product there passes through `mul`, an exclusive-or of its bits
 * with a word that is zero and known only when the shader runs, so that
 * each operation is rounded on its own. */
static const char *SHAPE_WGSL =
    "struct U {\n"
    "  inv: vec2<f32>,\n"    /* points a pixel, width / pixel_width */
    "  scale: vec2<f32>,\n"  /* pixels a point, for the rectangle of pixels */
    "  pixels: vec2<f32>,\n" /* the frame's size in pixels */
    "  zbits: u32,\n"        /* zero */
    "  pad: u32,\n"
    "};\n"
    "@group(0) @binding(0) var<uniform> u: U;\n"
    "\n"
    "struct V {\n"
    "  @builtin(position) pos: vec4<f32>,\n"
    "  @location(0) @interpolate(flat) a: vec4<u32>,\n"
    "  @location(1) @interpolate(flat) b: vec4<u32>,\n"
    "};\n"
    "\n"
    "fn is_nan(bits: u32) -> bool {\n"
    "  return (bits & 0x7f800000u) == 0x7f800000u && (bits & 0x007fffffu) != 0u;\n"
    "}\n"
    "\n"
    "@vertex\n"
    "fn vs(@builtin(vertex_index) vi: u32, @location(0) a: vec4<u32>,\n"
    "      @location(1) b: vec4<u32>) -> V {\n"
    "  let kind = a.x;\n"
    "  let p = array<f32, 6>(bitcast<f32>(a.y), bitcast<f32>(a.z), bitcast<f32>(a.w),\n"
    "                        bitcast<f32>(b.x), bitcast<f32>(b.y), bitcast<f32>(b.z));\n"
    "  var shown = kind >= 1u && kind <= 4u && !(is_nan(a.y) || is_nan(a.z) ||\n"
    "    is_nan(a.w) || is_nan(b.x) || is_nan(b.y) || is_nan(b.z));\n"
    "  var lo = vec2<f32>(0.0, 0.0);\n"
    "  var hi = vec2<f32>(0.0, 0.0);\n"
    "  switch kind {\n"
    "    case 1u: {\n"
    "      shown = shown && p[2] > 0.0 && p[3] > 0.0;\n"
    "      lo = vec2<f32>(p[0], p[1]);\n"
    "      hi = vec2<f32>(p[0] + p[2], p[1] + p[3]);\n"
    "    }\n"
    "    case 2u: {\n"
    "      shown = shown && p[2] > 0.0;\n"
    "      lo = vec2<f32>(p[0] - p[2], p[1] - p[2]);\n"
    "      hi = vec2<f32>(p[0] + p[2], p[1] + p[2]);\n"
    "    }\n"
    "    case 3u: {\n"
    "      shown = shown && p[4] > 0.0;\n"
    "      let h = p[4] * 0.5;\n"
    "      lo = vec2<f32>(min(p[0], p[2]) - h, min(p[1], p[3]) - h);\n"
    "      hi = vec2<f32>(max(p[0], p[2]) + h, max(p[1], p[3]) + h);\n"
    "    }\n"
    "    case 4u: {\n"
    "      lo = vec2<f32>(min(min(p[0], p[2]), p[4]), min(min(p[1], p[3]), p[5]));\n"
    "      hi = vec2<f32>(max(max(p[0], p[2]), p[4]), max(max(p[1], p[3]), p[5]));\n"
    "    }\n"
    "    default: {\n"
    "    }\n"
    "  }\n"
    "  let plo = clamp(floor(lo * u.scale) - vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 0.0), u.pixels);\n"
    "  let phi = clamp(ceil(hi * u.scale) + vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 0.0), u.pixels);\n"
    "  let k = vec2<bool>(vi == 1u || vi == 4u || vi == 5u, vi == 2u || vi == 3u || vi == 5u);\n"
    "  let px = select(plo, phi, k);\n"
    "  var o: V;\n"
    "  o.pos = vec4<f32>(px.x * 2.0 / u.pixels.x - 1.0, 1.0 - px.y * 2.0 / u.pixels.y, 0.0, 1.0);\n"
    "  if (!shown) {\n"
    "    o.pos = vec4<f32>(2.0, 2.0, 0.0, 1.0);\n"
    "  }\n"
    "  o.a = a;\n"
    "  o.b = b;\n"
    "  return o;\n"
    "}\n"
    "\n"
    "fn mul(x: f32, y: f32) -> f32 {\n"
    "  return bitcast<f32>(bitcast<u32>(x * y) ^ u.zbits);\n"
    "}\n"
    "\n"
    /* Whether a center exactly on an edge of direction d, with the inside
     * on its right, counts: a left edge or a top edge, y down. */
    "fn on_edge_counts(d: vec2<f32>) -> bool {\n"
    "  return d.y < 0.0 || (d.y == 0.0 && d.x > 0.0);\n"
    "}\n"
    "\n"
    /* The test of the edge from a to b at c, computed from the edge's two
     * ends in one order whichever way it runs. */
    "fn edge(a: vec2<f32>, b: vec2<f32>, c: vec2<f32>) -> bool {\n"
    "  var e: f32;\n"
    "  if (b.x < a.x || (b.x == a.x && b.y < a.y)) {\n"
    "    let d = a - b;\n"
    "    let q = c - b;\n"
    "    e = -(mul(d.x, q.y) - mul(d.y, q.x));\n"
    "  } else {\n"
    "    let d = b - a;\n"
    "    let q = c - a;\n"
    "    e = mul(d.x, q.y) - mul(d.y, q.x);\n"
    "  }\n"
    "  return e > 0.0 || (e == 0.0 && on_edge_counts(b - a));\n"
    "}\n"
    "\n"
    "@fragment\n"
    "fn fs(v: V) -> @location(0) vec4<f32> {\n"
    "  let c = vec2<f32>(mul(v.pos.x, u.inv.x), mul(v.pos.y, u.inv.y));\n"
    "  let n0 = bitcast<f32>(v.a.y);\n"
    "  let n1 = bitcast<f32>(v.a.z);\n"
    "  let n2 = bitcast<f32>(v.a.w);\n"
    "  let n3 = bitcast<f32>(v.b.x);\n"
    "  let n4 = bitcast<f32>(v.b.y);\n"
    "  let n5 = bitcast<f32>(v.b.z);\n"
    "  var inside = false;\n"
    "  switch v.a.x {\n"
    "    case 1u: {\n"
    "      inside = n0 <= c.x && c.x < n0 + n2 && n1 <= c.y && c.y < n1 + n3;\n"
    "    }\n"
    "    case 2u: {\n"
    "      let dx = c.x - n0;\n"
    "      let dy = c.y - n1;\n"
    "      inside = mul(dx, dx) + mul(dy, dy) < mul(n2, n2);\n"
    "    }\n"
    "    case 3u: {\n"
    "      let a = vec2<f32>(n0, n1);\n"
    "      let b = vec2<f32>(n2, n3);\n"
    "      let d = b - a;\n"
    "      let q = c - a;\n"
    "      let t = mul(q.x, d.x) + mul(q.y, d.y);\n"
    "      let s = mul(q.x, d.y) - mul(q.y, d.x);\n"
    "      let l = mul(d.x, d.x) + mul(d.y, d.y);\n"
    "      let lhs = mul(4.0 * s, s);\n"
    "      let rhs = mul(mul(n4, n4), l);\n"
    "      var body = false;\n"
    "      if (l > 0.0 && 0.0 <= t && t <= l) {\n"
    "        if (lhs < rhs) {\n"
    "          body = true;\n"
    "        } else if (lhs == rhs) {\n"
    "          if (s > 0.0) {\n"
    "            body = on_edge_counts(d);\n"
    "          } else if (s < 0.0) {\n"
    "            body = on_edge_counts(-d);\n"
    "          }\n"
    "        }\n"
    "      }\n"
    "      let h = n4 * 0.5;\n"
    "      let r2 = mul(h, h);\n"
    "      let e = c - b;\n"
    "      inside = body || mul(q.x, q.x) + mul(q.y, q.y) < r2 || mul(e.x, e.x) + mul(e.y, e.y) < r2;\n"
    "    }\n"
    "    case 4u: {\n"
    "      let p1 = vec2<f32>(n0, n1);\n"
    "      var p2 = vec2<f32>(n2, n3);\n"
    "      var p3 = vec2<f32>(n4, n5);\n"
    "      let w = mul(p2.x - p1.x, p3.y - p1.y) - mul(p2.y - p1.y, p3.x - p1.x);\n"
    "      if (w < 0.0) {\n"
    "        let swap = p2;\n"
    "        p2 = p3;\n"
    "        p3 = swap;\n"
    "      }\n"
    "      inside = w != 0.0 && edge(p1, p2, c) && edge(p2, p3, c) && edge(p3, p1, c);\n"
    "    }\n"
    "    default: {\n"
    "    }\n"
    "  }\n"
    "  if (!inside) {\n"
    "    discard;\n"
    "  }\n"
    "  return unpack4x8unorm(v.b.w);\n"
    "}\n";

static WGPUShaderModule shader_module(const char *code) {
    WGPUShaderSourceWGSL source = {0};
    source.chain.sType = WGPUSType_ShaderSourceWGSL;
    source.code = view(code);
    WGPUShaderModuleDescriptor descriptor = {0};
    descriptor.nextInChain = &source.chain;
    return wgpuDeviceCreateShaderModule(mn_gpu_device(), &descriptor);
}

/* Makes the shape pipeline, its uniform buffer and their binding, unless
 * they are made. */
static void shapes_make(void) {
    if (win.shapes) return;
    WGPUDevice device = mn_gpu_device();
    WGPUShaderModule module = shader_module(SHAPE_WGSL);

    WGPUBufferDescriptor uniform_descriptor = {0};
    uniform_descriptor.usage = WGPUBufferUsage_Uniform | WGPUBufferUsage_CopyDst;
    uniform_descriptor.size = 32;
    win.shape_uniform = wgpuDeviceCreateBuffer(device, &uniform_descriptor);
    if (!win.shape_uniform) window_fail("can't make a buffer of 32 bytes");

    WGPUBindGroupLayoutEntry entry = {0};
    entry.binding = 0;
    entry.visibility = WGPUShaderStage_Vertex | WGPUShaderStage_Fragment;
    entry.buffer.type = WGPUBufferBindingType_Uniform;
    entry.buffer.minBindingSize = 32;
    WGPUBindGroupLayoutDescriptor layout_descriptor = {0};
    layout_descriptor.entryCount = 1;
    layout_descriptor.entries = &entry;
    WGPUBindGroupLayout layout = wgpuDeviceCreateBindGroupLayout(device, &layout_descriptor);
    WGPUPipelineLayoutDescriptor pipeline_layout_descriptor = {0};
    pipeline_layout_descriptor.bindGroupLayoutCount = 1;
    pipeline_layout_descriptor.bindGroupLayouts = &layout;
    WGPUPipelineLayout pipeline_layout =
        wgpuDeviceCreatePipelineLayout(device, &pipeline_layout_descriptor);

    WGPUBindGroupEntry bind = {0};
    bind.binding = 0;
    bind.buffer = win.shape_uniform;
    bind.size = 32;
    WGPUBindGroupDescriptor group_descriptor = {0};
    group_descriptor.layout = layout;
    group_descriptor.entryCount = 1;
    group_descriptor.entries = &bind;
    win.shape_group = wgpuDeviceCreateBindGroup(device, &group_descriptor);

    /* A record: the kind and the first three numbers, then the other three
     * and the color, each as 32-bit words. */
    WGPUVertexAttribute attributes[2] = {0};
    attributes[0].format = WGPUVertexFormat_Uint32x4;
    attributes[0].offset = 0;
    attributes[0].shaderLocation = 0;
    attributes[1].format = WGPUVertexFormat_Uint32x4;
    attributes[1].offset = 16;
    attributes[1].shaderLocation = 1;
    WGPUVertexBufferLayout records = {0};
    records.stepMode = WGPUVertexStepMode_Instance;
    records.arrayStride = sizeof(MnDraw);
    records.attributeCount = 2;
    records.attributes = attributes;

    /* The color over the frame by its alpha; the frame's alpha stays. */
    WGPUBlendState blend = {0};
    blend.color.operation = WGPUBlendOperation_Add;
    blend.color.srcFactor = WGPUBlendFactor_SrcAlpha;
    blend.color.dstFactor = WGPUBlendFactor_OneMinusSrcAlpha;
    blend.alpha.operation = WGPUBlendOperation_Add;
    blend.alpha.srcFactor = WGPUBlendFactor_Zero;
    blend.alpha.dstFactor = WGPUBlendFactor_One;
    WGPUColorTargetState target = {0};
    target.format = WGPUTextureFormat_RGBA8Unorm;
    target.blend = &blend;
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
    descriptor.vertex.bufferCount = 1;
    descriptor.vertex.buffers = &records;
    descriptor.primitive.topology = WGPUPrimitiveTopology_TriangleList;
    descriptor.primitive.cullMode = WGPUCullMode_None;
    descriptor.multisample.count = 1;
    descriptor.multisample.mask = ~0u;
    descriptor.fragment = &fragment;
    win.shapes = wgpuDeviceCreateRenderPipeline(device, &descriptor);
    if (!win.shapes) window_fail("can't build the pipeline that draws shapes");
    wgpuPipelineLayoutRelease(pipeline_layout);
    wgpuBindGroupLayoutRelease(layout);
    wgpuShaderModuleRelease(module);
}

static void shapes_drop(void) {
    if (win.shape_group) wgpuBindGroupRelease(win.shape_group);
    if (win.shapes) wgpuRenderPipelineRelease(win.shapes);
    if (win.shape_uniform) {
        wgpuBufferDestroy(win.shape_uniform);
        wgpuBufferRelease(win.shape_uniform);
    }
    if (win.records) {
        wgpuBufferDestroy(win.records);
        wgpuBufferRelease(win.records);
    }
    win.shape_group = NULL;
    win.shapes = NULL;
    win.shape_uniform = NULL;
    win.records = NULL;
    win.records_capacity = 0;
}

/* Makes the buffer of records hold `count` records, unless it does. */
static void records_size(uint64_t count) {
    if (win.records_capacity >= count) return;
    if (win.records) {
        wgpuBufferDestroy(win.records);
        wgpuBufferRelease(win.records);
    }
    WGPUBufferDescriptor descriptor = {0};
    descriptor.usage = WGPUBufferUsage_Vertex | WGPUBufferUsage_CopyDst;
    descriptor.size = count * sizeof(MnDraw);
    win.records = wgpuDeviceCreateBuffer(mn_gpu_device(), &descriptor);
    if (!win.records) {
        window_fail("can't make a buffer of %llu bytes",
                    (unsigned long long)descriptor.size);
    }
    win.records_capacity = count;
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
    descriptor.format = WGPUTextureFormat_RGBA8Unorm;
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

/* Begins a render pass into the frame texture, cleared to `clear` or, when
 * that is NULL, keeping what it holds. */
static WGPURenderPassEncoder frame_pass(WGPUCommandEncoder encoder,
                                        const MnColor *clear) {
    WGPURenderPassColorAttachment attachment = {0};
    attachment.view = win.view;
    attachment.depthSlice = WGPU_DEPTH_SLICE_UNDEFINED;
    attachment.loadOp = clear ? WGPULoadOp_Clear : WGPULoadOp_Load;
    attachment.storeOp = WGPUStoreOp_Store;
    if (clear) {
        attachment.clearValue = (WGPUColor){clear->r / 255.0, clear->g / 255.0,
                                            clear->b / 255.0, 1.0};
    }
    WGPURenderPassDescriptor pass_descriptor = {0};
    pass_descriptor.colorAttachmentCount = 1;
    pass_descriptor.colorAttachments = &attachment;
    WGPURenderPassEncoder pass =
        wgpuCommandEncoderBeginRenderPass(encoder, &pass_descriptor);
    wgpuRenderPassEncoderSetPipeline(pass, win.shapes);
    wgpuRenderPassEncoderSetBindGroup(pass, 0, win.shape_group, 0, NULL);
    return pass;
}

/* Records the rendering of `frame` into the frame texture: the background,
 * then each span's records in order, those of a GPU block from the buffer
 * the block wrote them to. Records in CPU memory are sent to the buffer of
 * records at places no recorded command reads; when the frame has more
 * than the buffer holds, the commands so far are submitted and recording
 * goes on in a new encoder, which is returned. */
static WGPUCommandEncoder frame_record(WGPUCommandEncoder encoder,
                                       const MnFrame *frame) {
    frame_texture_size(frame->width, frame->height);
    shapes_make();
    float uniform[8] = {0};
    uniform[0] = (float)frame->point_width / (float)frame->width;
    uniform[1] = (float)frame->point_height / (float)frame->height;
    uniform[2] = (float)frame->width / (float)frame->point_width;
    uniform[3] = (float)frame->height / (float)frame->point_height;
    uniform[4] = (float)frame->width;
    uniform[5] = (float)frame->height;
    wgpuQueueWriteBuffer(mn_gpu_queue(), win.shape_uniform, 0, uniform, sizeof uniform);

    uint64_t total = 0;
    for (size_t s = 0; s < frame->span_count; s++) {
        if (frame->spans[s].records) total += frame->spans[s].count;
    }
    uint64_t most = mn_gpu_largest_buffer() / sizeof(MnDraw);
    if (most > total) most = total;
    if (most != 0) records_size(most);

    WGPURenderPassEncoder pass = frame_pass(encoder, &frame->background);
    uint64_t used = 0;
    for (size_t s = 0; s < frame->span_count; s++) {
        const MnSpan *span = &frame->spans[s];
        if (!span->records) {
            /* Records a block wrote on the device are drawn where they are. */
            uint64_t bytes = (uint64_t)span->count * sizeof(MnDraw);
            if (bytes == 0) continue;
            wgpuRenderPassEncoderSetVertexBuffer(pass, 0, mn_gpu_records(&span->on_device),
                                                 0, bytes);
            wgpuRenderPassEncoderDraw(pass, 6, (uint32_t)span->count, 0, 0);
            continue;
        }
        if (span->count != 0) mn_gpu_uploads++;
        size_t done = 0;
        while (done < span->count) {
            if (used == win.records_capacity) {
                wgpuRenderPassEncoderEnd(pass);
                wgpuRenderPassEncoderRelease(pass);
                mn_gpu_device_submit_commands(wgpuCommandEncoderFinish(encoder, NULL),
                                              false);
                wgpuCommandEncoderRelease(encoder);
                encoder = wgpuDeviceCreateCommandEncoder(mn_gpu_device(), NULL);
                pass = frame_pass(encoder, NULL);
                used = 0;
            }
            uint64_t count = span->count - done;
            if (count > win.records_capacity - used) count = win.records_capacity - used;
            uint64_t offset = used * sizeof(MnDraw);
            uint64_t bytes = count * sizeof(MnDraw);
            wgpuQueueWriteBuffer(mn_gpu_queue(), win.records, offset,
                                 span->records + done, (size_t)bytes);
            wgpuRenderPassEncoderSetVertexBuffer(pass, 0, win.records, offset, bytes);
            wgpuRenderPassEncoderDraw(pass, 6, (uint32_t)count, 0, 0);
            used += count;
            done += (size_t)count;
        }
    }
    wgpuRenderPassEncoderEnd(pass);
    wgpuRenderPassEncoderRelease(pass);
    win.drawn = frame->number;
    win.holds = true;
    return encoder;
}

/* ---- Showing the frame on the surface ------------------------------------ */

/* A triangle that covers the target; each pixel is the frame's pixel at
 * the same place, as it is. */
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
    WGPUShaderModule module = shader_module(BLIT_WGSL);

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

/* Submits `encoder`'s commands as one submission. */
static void submit(WGPUCommandEncoder encoder) {
    mn_gpu_device_submit_commands(wgpuCommandEncoderFinish(encoder, NULL), true);
    wgpuCommandEncoderRelease(encoder);
}

static void frame_size_check(const MnFrame *frame) {
    if (frame->width > MN_WINDOW_LARGEST || frame->height > MN_WINDOW_LARGEST) {
        window_fail("the desktop made the window %ux%u pixels, larger than %d by %d",
                    frame->width, frame->height, MN_WINDOW_LARGEST,
                    MN_WINDOW_LARGEST);
    }
}

void mn_window_draw(const MnFrame *frame) {
    if (frame->width == 0 || frame->height == 0) return;
    if (!win.window || win.hidden) return;
    frame_size_check(frame);
    mn_gpu_device_wait();
    WGPUCommandEncoder encoder =
        frame_record(wgpuDeviceCreateCommandEncoder(mn_gpu_device(), NULL), frame);
    WGPUSurfaceTexture target = {0};
    if (win.config.width == frame->width && win.config.height == frame->height) {
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
    frame_size_check(frame);
    WGPUDevice device = mn_gpu_device();
    WGPUCommandEncoder encoder = wgpuDeviceCreateCommandEncoder(device, NULL);
    if (!win.holds || win.drawn != frame->number || win.width != frame->width ||
        win.height != frame->height) {
        encoder = frame_record(encoder, frame);
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
    mn_gpu_device_wait();

    int done = 0;
    WGPUBufferMapCallbackInfo callback = {0};
    callback.mode = WGPUCallbackMode_AllowProcessEvents;
    callback.callback = on_map;
    callback.userdata1 = &done;
    wgpuBufferMapAsync(win.readback, WGPUMapMode_Read, 0, (size_t)size, callback);
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
 * the first of RGBA8Unorm and BGRA8Unorm, the first of mailbox, immediate and FIFO,
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
    static const WGPUTextureFormat formats[] = {WGPUTextureFormat_RGBA8Unorm,
                                                WGPUTextureFormat_BGRA8Unorm};
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

static void key_table_make(void);

const char *mn_window_show(const char *title, int64_t width, int64_t height,
                           MnWindowSizes *sizes, const char **how) {
    if (!display_named()) return "no display was found";
    if (!win.sdl) {
#ifndef __APPLE__
        SDL_SetHint(SDL_HINT_VIDEO_DRIVER, "wayland,x11");
#endif
        SDL_SetHint(SDL_HINT_NO_SIGNAL_HANDLERS, "1");
        SDL_SetHint(SDL_HINT_TOUCH_MOUSE_EVENTS, "0");
        SDL_SetHint(SDL_HINT_PEN_MOUSE_EVENTS, "0");
        SDL_SetHint(SDL_HINT_MOUSE_FOCUS_CLICKTHROUGH, "1");
        if (!SDL_InitSubSystem(SDL_INIT_VIDEO)) return "no display was found";
        key_table_make();
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

/* The number of the `Key` variant of each SDL scancode below 256, plus
 * one; 0 for a scancode without a `Key`. */
static uint8_t key_of_scancode[256];

static void key_table_make(void) {
    uint8_t key = 0;
#define MN_KEY_SCANCODE(name, usage) key_of_scancode[usage] = ++key;
    MN_KEYS(MN_KEY_SCANCODE)
#undef MN_KEY_SCANCODE
}

/* Whether a mouse event is made by SDL from touch or a pen. */
static bool from_touch_or_pen(SDL_MouseID which) {
    return which == SDL_TOUCH_MOUSEID || which == SDL_PEN_MOUSEID;
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
        case SDL_EVENT_KEY_DOWN:
        case SDL_EVENT_KEY_UP: {
            SDL_Scancode code = event.key.scancode;
            if (event.key.repeat || code < 0 || code >= 256) break;
            if (key_of_scancode[code] == 0) break;
            mn_window_key(key_of_scancode[code] - 1u, event.type == SDL_EVENT_KEY_DOWN);
            break;
        }
        case SDL_EVENT_MOUSE_BUTTON_DOWN:
        case SDL_EVENT_MOUSE_BUTTON_UP:
            if (from_touch_or_pen(event.button.which)) break;
            if (event.button.button < 1 || event.button.button > MN_MOUSE_BUTTON_COUNT) {
                break;
            }
            mn_window_button(event.button.button - 1u,
                             event.type == SDL_EVENT_MOUSE_BUTTON_DOWN, event.button.x,
                             event.button.y);
            break;
        case SDL_EVENT_MOUSE_MOTION:
            if (from_touch_or_pen(event.motion.which)) break;
            mn_window_move(event.motion.x, event.motion.y);
            break;
        case SDL_EVENT_MOUSE_WHEEL:
            if (from_touch_or_pen(event.wheel.which)) break;
            mn_window_wheel(event.wheel.x, event.wheel.y);
            break;
        case SDL_EVENT_WINDOW_MOUSE_LEAVE:
            mn_window_leave();
            break;
        case SDL_EVENT_WINDOW_FOCUS_GAINED:
        case SDL_EVENT_WINDOW_FOCUS_LOST:
            mn_window_focus(event.type == SDL_EVENT_WINDOW_FOCUS_GAINED);
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
    shapes_drop();
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
