/* Runs the `/` and `sqrt` of strict shaders on the device and compares
 * every result with the CPU's `/` and `sqrtf`, subnormals flushed.
 *
 *     strict_div_sqrt HELPERS SQRT_STRIDE DIV_PAIRS
 *
 * HELPERS is a file with the WGSL functions `s32_div` and `s32_sqrt`.
 * `sqrt` runs on every bit pattern when SQRT_STRIDE is 1, and otherwise on
 * one pattern in each run of SQRT_STRIDE. `/` runs on DIV_PAIRS pairs: random
 * bit patterns, divisors at and above 2^126, and quotients next to a
 * midpoint (a divisor whose significand is a little above or below the
 * dividend's), and then on every pair of a table of edge values. Prints the
 * device, the counts and the first mismatches; the status is 0 when every
 * result matches. The device is the one Vulkan finds, so VK_DRIVER_FILES
 * chooses it. */
#define _POSIX_C_SOURCE 200809L
#include <math.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <xmmintrin.h>

#include <webgpu/webgpu.h>
#include <webgpu/wgpu.h>

/* Rows per dispatch: the most one dimension of workgroups reaches. */
#define CHUNK (65535u * 64u)

static const uint32_t EDGES[] = {
    0x00000000, 0x80000000, 0x00000001, 0x807fffff, 0x007fffff, 0x00800000, 0x80800000,
    0x00800001, 0x3f800000, 0xbf800000, 0x3f7fffff, 0x3f800001, 0x40000000, 0x3fffffff,
    0x7f7fffff, 0xff7fffff, 0x7f000000, 0x7e800000, 0x7f800000, 0xff800000, 0x7fc00000,
    0xffc00001, 0x7f800001, 0x00ffffff, 0x01000000, 0x0c000000, 0x33800000, 0x4b000000,
    0x4b7fffff, 0x5f000000, 0x20000000, 0x1f800000, 0x3eaaaaab, 0x3f2aaaab, 0x40490fdb,
    0x3fb504f3, 0x3fb504f4, 0x4e6e6b28, 0x2f800000, 0x7effffff};
#define EDGE_COUNT (sizeof EDGES / sizeof EDGES[0])

static const char *KERNEL =
    "struct Params {\n"
    "    base: u32,\n"
    "    kind: u32,\n"
    "    stride: u32,\n"
    "    count: u32,\n"
    "}\n"
    "@group(0) @binding(0) var<uniform> p: Params;\n"
    "@group(0) @binding(1) var<storage, read_write> results: array<u32>;\n"
    "@group(0) @binding(2) var<storage, read> edges: array<u32>;\n"
    "fn hash(x: u32) -> u32 {\n"
    "    var h = x;\n"
    "    h = h ^ (h >> 16u);\n"
    "    h = h * 0x7feb352du;\n"
    "    h = h ^ (h >> 15u);\n"
    "    h = h * 0x846ca68bu;\n"
    "    h = h ^ (h >> 16u);\n"
    "    return h;\n"
    "}\n"
    "fn sqrt_input(k: u32) -> u32 {\n"
    "    if (p.stride == 1u) {\n"
    "        return k;\n"
    "    }\n"
    "    return k * p.stride + hash(k) % p.stride;\n"
    "}\n"
    "fn div_inputs(k: u32) -> vec2<u32> {\n"
    "    if (p.kind == 2u) {\n"
    "        let n = arrayLength(&edges);\n"
    "        return vec2<u32>(edges[k / n], edges[k % n]);\n"
    "    }\n"
    "    let a = hash(2u * k);\n"
    "    let h = hash(2u * k + 1u);\n"
    "    if ((k & 1u) == 1u) {\n"
    "        return vec2<u32>(a, (a & 0x7fffffffu) + (h & 7u) - 3u);\n"
    "    }\n"
    "    if ((k & 3u) == 2u) {\n"
    "        return vec2<u32>(a, (h & 0x807fffffu) | 0x7e800000u);\n"
    "    }\n"
    "    return vec2<u32>(a, h);\n"
    "}\n"
    "@compute @workgroup_size(64)\n"
    "fn main(@builtin(global_invocation_id) gid: vec3<u32>) {\n"
    "    let i = gid.x;\n"
    "    if (i >= p.count) {\n"
    "        return;\n"
    "    }\n"
    "    let k = p.base + i;\n"
    "    if (p.kind == 0u) {\n"
    "        results[i] = bitcast<u32>(s32_sqrt(bitcast<f32>(sqrt_input(k))));\n"
    "        return;\n"
    "    }\n"
    "    let ab = div_inputs(k);\n"
    "    results[i] = bitcast<u32>(s32_div(bitcast<f32>(ab.x), bitcast<f32>(ab.y)));\n"
    "}\n";

static uint32_t hash(uint32_t x) {
    uint32_t h = x;
    h ^= h >> 16;
    h *= 0x7feb352du;
    h ^= h >> 15;
    h *= 0x846ca68bu;
    h ^= h >> 16;
    return h;
}

static float as_float(uint32_t bits) {
    float value;
    memcpy(&value, &bits, sizeof value);
    return value;
}

static uint32_t as_bits(float value) {
    uint32_t bits;
    memcpy(&bits, &value, sizeof bits);
    return bits;
}

static bool is_nan(uint32_t bits) { return (bits & 0x7fffffffu) > 0x7f800000u; }

static WGPUInstance instance;
static WGPUDevice device;
static WGPUQueue queue;
static int device_errors;

static WGPUStringView view(const char *text) { return (WGPUStringView){text, WGPU_STRLEN}; }

static void on_adapter(WGPURequestAdapterStatus status, WGPUAdapter adapter,
                       WGPUStringView message, void *out, void *unused) {
    (void)message;
    (void)unused;
    if (status == WGPURequestAdapterStatus_Success) *(WGPUAdapter *)out = adapter;
}

static void on_device(WGPURequestDeviceStatus status, WGPUDevice got, WGPUStringView message,
                      void *out, void *unused) {
    (void)message;
    (void)unused;
    if (status == WGPURequestDeviceStatus_Success) *(WGPUDevice *)out = got;
}

static void on_error(WGPUDevice const *from, WGPUErrorType type, WGPUStringView message,
                     void *unused1, void *unused2) {
    (void)from;
    (void)unused1;
    (void)unused2;
    device_errors++;
    size_t length = message.length == WGPU_STRLEN ? strlen(message.data) : message.length;
    fprintf(stderr, "device error %d: %.*s\n", (int)type, (int)length, message.data);
}

static void on_map(WGPUMapAsyncStatus status, WGPUStringView message, void *done,
                   void *unused) {
    (void)message;
    (void)unused;
    *(int *)done = status == WGPUMapAsyncStatus_Success ? 1 : -1;
}

static void start(void) {
    instance = wgpuCreateInstance(NULL);
    WGPUAdapter adapter = NULL;
    WGPURequestAdapterOptions options = {0};
    options.featureLevel = WGPUFeatureLevel_Core;
    options.powerPreference = WGPUPowerPreference_HighPerformance;
    WGPURequestAdapterCallbackInfo adapter_callback = {0};
    adapter_callback.mode = WGPUCallbackMode_AllowSpontaneous;
    adapter_callback.callback = on_adapter;
    adapter_callback.userdata1 = &adapter;
    wgpuInstanceRequestAdapter(instance, &options, adapter_callback);
    wgpuInstanceProcessEvents(instance);
    if (!adapter) {
        printf("no device\n");
        exit(2);
    }
    WGPUAdapterInfo info = {0};
    wgpuAdapterGetInfo(adapter, &info);
    printf("device: %.*s\n", (int)info.device.length, info.device.data);
    wgpuAdapterInfoFreeMembers(info);
    WGPUDeviceDescriptor descriptor = {0};
    descriptor.uncapturedErrorCallbackInfo.callback = on_error;
    WGPURequestDeviceCallbackInfo device_callback = {0};
    device_callback.mode = WGPUCallbackMode_AllowSpontaneous;
    device_callback.callback = on_device;
    device_callback.userdata1 = &device;
    wgpuAdapterRequestDevice(adapter, &descriptor, device_callback);
    wgpuInstanceProcessEvents(instance);
    wgpuAdapterRelease(adapter);
    if (!device) {
        printf("no device\n");
        exit(2);
    }
    queue = wgpuDeviceGetQueue(device);
}

static WGPUBuffer buffer(uint64_t size, WGPUBufferUsage usage) {
    WGPUBufferDescriptor descriptor = {0};
    descriptor.usage = usage;
    descriptor.size = size;
    WGPUBuffer made = wgpuDeviceCreateBuffer(device, &descriptor);
    if (!made) {
        printf("can't create a buffer of %llu bytes\n", (unsigned long long)size);
        exit(2);
    }
    return made;
}

static char *read_file(const char *path) {
    FILE *file = fopen(path, "rb");
    if (!file) {
        perror(path);
        exit(2);
    }
    fseek(file, 0, SEEK_END);
    long size = ftell(file);
    fseek(file, 0, SEEK_SET);
    char *text = malloc((size_t)size + 1);
    if (!text || fread(text, 1, (size_t)size, file) != (size_t)size) exit(2);
    text[size] = 0;
    fclose(file);
    return text;
}

static WGPUComputePipeline pipeline;
static WGPUBindGroup group;
static WGPUBuffer params, results, readback;

/* Runs `count` rows of `kind` from `base` and maps their results. */
static const uint32_t *run(uint32_t kind, uint32_t base, uint32_t stride, uint32_t count) {
    uint32_t words[4] = {base, kind, stride, count};
    wgpuQueueWriteBuffer(queue, params, 0, words, sizeof words);
    WGPUCommandEncoder encoder = wgpuDeviceCreateCommandEncoder(device, NULL);
    WGPUComputePassEncoder pass = wgpuCommandEncoderBeginComputePass(encoder, NULL);
    wgpuComputePassEncoderSetPipeline(pass, pipeline);
    wgpuComputePassEncoderSetBindGroup(pass, 0, group, 0, NULL);
    wgpuComputePassEncoderDispatchWorkgroups(pass, (count + 63) / 64, 1, 1);
    wgpuComputePassEncoderEnd(pass);
    wgpuComputePassEncoderRelease(pass);
    wgpuCommandEncoderCopyBufferToBuffer(encoder, results, 0, readback, 0, (uint64_t)count * 4);
    WGPUCommandBuffer commands = wgpuCommandEncoderFinish(encoder, NULL);
    wgpuQueueSubmit(queue, 1, &commands);
    wgpuCommandBufferRelease(commands);
    wgpuCommandEncoderRelease(encoder);
    int done = 0;
    WGPUBufferMapCallbackInfo callback = {0};
    callback.mode = WGPUCallbackMode_AllowProcessEvents;
    callback.callback = on_map;
    callback.userdata1 = &done;
    wgpuBufferMapAsync(readback, WGPUMapMode_Read, 0, (size_t)count * 4, callback);
    while (done == 0) wgpuDevicePoll(device, true, NULL);
    if (done < 0 || device_errors) {
        printf("the device failed\n");
        exit(2);
    }
    return wgpuBufferGetConstMappedRange(readback, 0, (size_t)count * 4);
}

static uint64_t wrong;

static void compare(const char *what, uint32_t a, uint32_t b, uint32_t expected, uint32_t got) {
    if (expected == got || (is_nan(expected) && is_nan(got))) return;
    if (wrong < 10) {
        if (b == UINT32_MAX)
            printf("%s %08x: cpu %08x device %08x\n", what, a, expected, got);
        else
            printf("%s %08x %08x: cpu %08x device %08x\n", what, a, b, expected, got);
    }
    wrong++;
}

static volatile float left, right;

static uint32_t cpu_sqrt(uint32_t a) {
    left = as_float(a);
    return as_bits(sqrtf(left));
}

static uint32_t cpu_div(uint32_t a, uint32_t b) {
    left = as_float(a);
    right = as_float(b);
    return as_bits(left / right);
}

static void div_inputs(uint32_t kind, uint32_t k, uint32_t *a, uint32_t *b) {
    if (kind == 2) {
        *a = EDGES[k / EDGE_COUNT];
        *b = EDGES[k % EDGE_COUNT];
        return;
    }
    *a = hash(2u * k);
    uint32_t h = hash(2u * k + 1u);
    if ((k & 1u) == 1u) {
        *b = (*a & 0x7fffffffu) + (h & 7u) - 3u;
    } else if ((k & 3u) == 2u) {
        *b = (h & 0x807fffffu) | 0x7e800000u;
    } else {
        *b = h;
    }
}

int main(int argc, char **argv) {
    if (argc != 4) {
        fprintf(stderr, "usage: strict_div_sqrt HELPERS SQRT_STRIDE DIV_PAIRS\n");
        return 2;
    }
    char *helpers = read_file(argv[1]);
    uint32_t stride = (uint32_t)strtoul(argv[2], NULL, 10);
    uint64_t div_pairs = strtoull(argv[3], NULL, 10);
    if (stride == 0 || div_pairs > UINT32_MAX) return 2;

    start();
    size_t length = strlen(helpers) + strlen(KERNEL) + 2;
    char *code = malloc(length);
    snprintf(code, length, "%s\n%s", helpers, KERNEL);
    WGPUShaderSourceWGSL source = {0};
    source.chain.sType = WGPUSType_ShaderSourceWGSL;
    source.code = view(code);
    WGPUShaderModuleDescriptor module_descriptor = {0};
    module_descriptor.nextInChain = &source.chain;
    WGPUShaderModule module = wgpuDeviceCreateShaderModule(device, &module_descriptor);
    WGPUComputePipelineDescriptor pipeline_descriptor = {0};
    pipeline_descriptor.compute.module = module;
    pipeline_descriptor.compute.entryPoint = view("main");
    pipeline = wgpuDeviceCreateComputePipeline(device, &pipeline_descriptor);
    if (!module || !pipeline || device_errors) {
        printf("can't build the shader\n");
        return 2;
    }

    params = buffer(16, WGPUBufferUsage_Uniform | WGPUBufferUsage_CopyDst);
    results = buffer((uint64_t)CHUNK * 4, WGPUBufferUsage_Storage | WGPUBufferUsage_CopySrc);
    readback = buffer((uint64_t)CHUNK * 4, WGPUBufferUsage_MapRead | WGPUBufferUsage_CopyDst);
    WGPUBuffer edges = buffer(sizeof EDGES, WGPUBufferUsage_Storage | WGPUBufferUsage_CopyDst);
    wgpuQueueWriteBuffer(queue, edges, 0, EDGES, sizeof EDGES);
    WGPUBindGroupEntry entries[3] = {0};
    entries[0] = (WGPUBindGroupEntry){.binding = 0, .buffer = params, .size = 16};
    entries[1] = (WGPUBindGroupEntry){.binding = 1, .buffer = results, .size = (uint64_t)CHUNK * 4};
    entries[2] = (WGPUBindGroupEntry){.binding = 2, .buffer = edges, .size = sizeof EDGES};
    WGPUBindGroupDescriptor group_descriptor = {0};
    group_descriptor.layout = wgpuComputePipelineGetBindGroupLayout(pipeline, 0);
    group_descriptor.entryCount = 3;
    group_descriptor.entries = entries;
    group = wgpuDeviceCreateBindGroup(device, &group_descriptor);

    unsigned int saved = _mm_getcsr();
    uint64_t sqrt_count = (0x100000000ull + stride - 1) / stride;
    for (uint64_t base = 0; base < sqrt_count; base += CHUNK) {
        uint32_t count = (uint32_t)(sqrt_count - base < CHUNK ? sqrt_count - base : CHUNK);
        const uint32_t *got = run(0, (uint32_t)base, stride, count);
        _mm_setcsr(saved | 0x8040u);
        for (uint32_t i = 0; i < count; i++) {
            uint32_t k = (uint32_t)base + i;
            uint32_t a = stride == 1 ? k : k * stride + hash(k) % stride;
            compare("sqrt", a, UINT32_MAX, cpu_sqrt(a), got[i]);
        }
        _mm_setcsr(saved);
        wgpuBufferUnmap(readback);
    }
    printf("sqrt: %llu inputs\n", (unsigned long long)sqrt_count);

    for (uint32_t kind = 1; kind <= 2; kind++) {
        uint64_t total = kind == 1 ? div_pairs : EDGE_COUNT * EDGE_COUNT;
        for (uint64_t base = 0; base < total; base += CHUNK) {
            uint32_t count = (uint32_t)(total - base < CHUNK ? total - base : CHUNK);
            const uint32_t *got = run(kind, (uint32_t)base, 1, count);
            _mm_setcsr(saved | 0x8040u);
            for (uint32_t i = 0; i < count; i++) {
                uint32_t a, b;
                div_inputs(kind, (uint32_t)base + i, &a, &b);
                compare("div", a, b, cpu_div(a, b), got[i]);
            }
            _mm_setcsr(saved);
            wgpuBufferUnmap(readback);
        }
    }
    printf("div: %llu pairs and %u edge pairs\n", (unsigned long long)div_pairs,
           (unsigned)(EDGE_COUNT * EDGE_COUNT));
    printf("%llu wrong\n", (unsigned long long)wrong);
    return wrong != 0;
}
