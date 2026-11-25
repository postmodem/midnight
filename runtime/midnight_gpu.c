/*
 * The part of the Midnight runtime that talks to wgpu-native. See
 * midnight_gpu.h for what each function promises, and the GPU systems
 * section of midnight_rt.h for how generated code describes a block.
 */

/* For RTLD_NEXT. */
#if defined(__linux__) && !defined(_GNU_SOURCE)
#define _GNU_SOURCE
#endif

#include "midnight_gpu.h"

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

#include <webgpu/webgpu.h>
#include <webgpu/wgpu.h>

#define COLUMN_USAGE                                                           \
    (WGPUBufferUsage_Storage | WGPUBufferUsage_CopySrc | WGPUBufferUsage_CopyDst)
#define SNAPSHOT_USAGE (WGPUBufferUsage_Storage | WGPUBufferUsage_CopyDst)
#define UNIFORM_USAGE (WGPUBufferUsage_Uniform | WGPUBufferUsage_CopyDst)
#define READBACK_USAGE (WGPUBufferUsage_MapRead | WGPUBufferUsage_CopyDst)

/* The largest number of workgroups a dispatch asks for in one dimension:
 * MN_GPU_ROW_SPAN rows. */
#define MAX_GROUPS 65535u

/* A wgpu buffer. No two buffers ever made share a serial, so a bind group
 * built from a buffer is still good exactly when the serial it recorded is
 * the buffer's. */
typedef struct Buffer {
    WGPUBuffer handle;
    uint64_t size;
    uint64_t serial;
    /* The serial of the last command encoder that named the buffer. */
    uint64_t used;
} Buffer;

enum { ON_CPU, ON_GPU, ON_BOTH };

/* The GPU copy of one column of one table. */
typedef struct Column {
    uint32_t component; /* a component id, or MN_GPU_ENTITIES */
    uint32_t size;      /* bytes per row */
    /* Which copy is current. */
    uint8_t where;
    Buffer buffer;
    /* Changes whenever the contents of `buffer` do. */
    uint64_t version;
    /* Listed for the next round trip. */
    bool fetching;
} Column;

struct MnGpuTable {
    Column *columns;
    uint32_t count;
    uint32_t capacity;
    /* How many columns are not ON_CPU. */
    uint32_t current;
};

/* A column of a table to bring back in the next round trip, and where in
 * the readback buffer it goes. */
typedef struct Fetch {
    MnTable *table;
    uint32_t component;
    uint64_t offset;
} Fetch;

static struct {
    WGPUInstance instance;
    WGPUDevice device;
    WGPUQueue queue;
    char name[160];
    /* The largest buffer, and the largest that can be bound for a shader
     * to read. */
    uint64_t max_buffer;
    uint64_t max_binding;
    uint64_t max_uniform;
    /* What the offset of uniform words in a buffer must be a multiple of. */
    uint32_t uniform_align;
    uint32_t max_storage_buffers;
    uint32_t max_groups;
    /* The last number given to a buffer or to a column's contents. */
    uint64_t serial;
    /* How many component columns, over all tables, are not ON_CPU, and
     * how many are ON_GPU. */
    uint64_t current;
    uint64_t alone;
    /* The columns to bring back in the next round trip. */
    Fetch *fetches;
    uint32_t fetch_count;
    uint32_t fetch_capacity;
    /* Where columns are copied to be read back. */
    Buffer readback;
    /* The command encoder that records work until it is submitted, and
     * the compute pass open in it; NULL when there is none. Each encoder
     * gets a serial of its own. */
    WGPUCommandEncoder encoder;
    WGPUComputePassEncoder pass;
    uint64_t encoder_serial;
    /* Whether anything has been submitted since the last wait. */
    bool pending;
    /* Set while the device is released, which reports it lost. */
    bool finishing;
    /* Working space: a snapshot put together in CPU memory, and the
     * description of a snapshot's contents. */
    unsigned char *bytes;
    uint64_t bytes_capacity;
    uint64_t *contents;
    uint32_t contents_capacity;
    /* Why the last block refused runs on the CPU. */
    char reason[200];
} gpu;

/* ---- Sanitizers ---------------------------------------------------------- */

#if defined(__has_feature)
#if __has_feature(address_sanitizer)
#define MN_GPU_LEAK_CHECKED
#endif
#elif defined(__SANITIZE_ADDRESS__)
#define MN_GPU_LEAK_CHECKED
#endif

#ifdef MN_GPU_LEAK_CHECKED
/* Read by LeakSanitizer. A GPU driver is a library that releasing the
 * device unloads, and some leave memory allocated behind them. A leak is
 * passed over when its allocation came through code that is no longer
 * loaded when the program ends; every other leak is still reported. */
const char *__lsan_default_suppressions(void);
const char *__lsan_default_suppressions(void) {
    return "leak:<unknown module>\n";
}

/* Keeps the table of suppressions used off standard error. */
const char *__lsan_default_options(void);
const char *__lsan_default_options(void) { return "print_suppressions=0"; }
#endif

#if defined(MN_GPU_LEAK_CHECKED) && defined(__linux__) &&                      \
    defined(__has_attribute)
#if __has_attribute(musttail)
#include <dlfcn.h>

/* AddressSanitizer's dlopen, when the runtime has one under this name. */
extern void *__interceptor_dlopen(const char *file, int mode)
    __attribute__((weak));

/* The program's dlopen, in place of AddressSanitizer's.
 *
 * The dynamic loader looks for a library named without a directory in the
 * runpath of whoever called dlopen. AddressSanitizer puts a dlopen of its
 * own in the executable, which calls the C library's, so the caller the
 * loader sees is always the executable. A GPU driver that opens its other
 * parts by name is then searched for in the wrong runpath, fails to start
 * wherever libraries are found through runpaths, and the program finds no
 * such device.
 *
 * This one jumps to the C library's dlopen without calling it, which
 * leaves the caller as it was. Two things AddressSanitizer's dlopen does
 * are kept:
 *
 * - It refuses RTLD_DEEPBIND, which it cannot work with. Such a call is
 *   handed to it.
 * - After the call it tells its symbolizer that the list of loaded
 *   libraries is out of date. Nothing can be done after a jump, so that is
 *   done before it, through AddressSanitizer's dlclose, which tells the
 *   symbolizer the same thing: the program itself is opened and closed.
 *   The list is read again when it is next needed. */
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

/* ---- Failures ------------------------------------------------------------ */

static _Noreturn void gpu_fail(const char *format, ...)
    __attribute__((format(printf, 1, 2)));

static _Noreturn void gpu_fail(const char *format, ...) {
    fflush(stdout);
    fputs("midnight: GPU error: ", stderr);
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

/* The length of a string view's text without the white space at its end. */
static int view_length(WGPUStringView text) {
    if (!text.data) return 0;
    size_t length = text.length == WGPU_STRLEN ? strlen(text.data) : text.length;
    while (length > 0 && (text.data[length - 1] == '\n' ||
                          text.data[length - 1] == ' ')) {
        length--;
    }
    return length > INT32_MAX ? INT32_MAX : (int)length;
}

static void on_error(WGPUDevice const *device, WGPUErrorType type,
                     WGPUStringView message, void *unused1, void *unused2) {
    (void)device;
    (void)type;
    (void)unused1;
    (void)unused2;
    gpu_fail("%.*s", view_length(message), message.data ? message.data : "");
}

static void on_lost(WGPUDevice const *device, WGPUDeviceLostReason reason,
                    WGPUStringView message, void *unused1, void *unused2) {
    (void)device;
    (void)reason;
    (void)unused1;
    (void)unused2;
    if (gpu.finishing) return;
    gpu_fail("the device was lost: %.*s", view_length(message),
             message.data ? message.data : "");
}

/* ---- Buffers ------------------------------------------------------------- */

static uint64_t round4(uint64_t bytes) { return (bytes + 3) & ~(uint64_t)3; }

/* Whether the open command encoder names the buffer. */
static bool buffer_in_use(const Buffer *buffer) {
    return gpu.encoder && buffer->used == gpu.encoder_serial;
}

/* Releases a buffer. One that the open encoder names is not destroyed: it
 * lives until the encoder's work is done. */
static void buffer_drop(Buffer *buffer) {
    if (buffer->handle) {
        if (!buffer_in_use(buffer)) wgpuBufferDestroy(buffer->handle);
        wgpuBufferRelease(buffer->handle);
    }
    buffer->handle = NULL;
    buffer->size = 0;
    buffer->serial = 0;
    buffer->used = 0;
}

/* Replaces `buffer` with a new, zeroed buffer of `size` bytes. */
static void buffer_make(Buffer *buffer, uint64_t size, WGPUBufferUsage usage) {
    buffer_drop(buffer);
    WGPUBufferDescriptor descriptor = {0};
    descriptor.usage = usage;
    descriptor.size = size;
    buffer->handle = wgpuDeviceCreateBuffer(gpu.device, &descriptor);
    if (!buffer->handle) {
        gpu_fail("can't create a buffer of %llu bytes", (unsigned long long)size);
    }
    buffer->size = size;
    buffer->serial = ++gpu.serial;
}

/* ---- Recording and submitting -------------------------------------------- */

/* Opens a command encoder if none is open. */
static void encoder_open(void) {
    if (gpu.encoder) return;
    gpu.encoder = wgpuDeviceCreateCommandEncoder(gpu.device, NULL);
    if (!gpu.encoder) gpu_fail("can't create a command encoder");
    gpu.encoder_serial = ++gpu.serial;
}

/* Records that the open encoder names a buffer. */
static void buffer_use(Buffer *buffer) { buffer->used = gpu.encoder_serial; }

/* Ends the open compute pass, if there is one. */
static void pass_end(void) {
    if (!gpu.pass) return;
    wgpuComputePassEncoderEnd(gpu.pass);
    wgpuComputePassEncoderRelease(gpu.pass);
    gpu.pass = NULL;
}

/* The open compute pass, begun if there is none. */
static WGPUComputePassEncoder pass_open(void) {
    encoder_open();
    if (!gpu.pass) {
        gpu.pass = wgpuCommandEncoderBeginComputePass(gpu.encoder, NULL);
    }
    return gpu.pass;
}

/* Records a copy of `bytes` bytes, a multiple of 4, from one buffer to
 * another. */
static void buffer_copy(Buffer *from, uint64_t from_offset, Buffer *to,
                        uint64_t to_offset, uint64_t bytes) {
    pass_end();
    encoder_open();
    wgpuCommandEncoderCopyBufferToBuffer(gpu.encoder, from->handle, from_offset,
                                         to->handle, to_offset, bytes);
    buffer_use(from);
    buffer_use(to);
}

/* Submits the open encoder, if there is one, without waiting. */
static void encoder_submit(void) {
    if (!gpu.encoder) return;
    pass_end();
    WGPUCommandBuffer commands = wgpuCommandEncoderFinish(gpu.encoder, NULL);
    wgpuQueueSubmit(gpu.queue, 1, &commands);
    mn_gpu_submissions++;
    wgpuCommandBufferRelease(commands);
    wgpuCommandEncoderRelease(gpu.encoder);
    gpu.encoder = NULL;
    gpu.pending = true;
}

void mn_gpu_device_submit(void) { encoder_submit(); }

/* Writes `bytes` bytes, a multiple of 4, at `offset` in a buffer through
 * the queue. Such a write takes effect before every command of the next
 * submission, whenever it was made, so the open encoder is submitted first
 * when it names the buffer. */
static void queue_write(Buffer *buffer, uint64_t offset, const void *data,
                        uint64_t bytes) {
    if (bytes == 0) return;
    if (buffer_in_use(buffer)) encoder_submit();
    wgpuQueueWriteBuffer(gpu.queue, buffer->handle, offset, data, (size_t)bytes);
}

/* Writes `bytes` bytes at `offset`, a multiple of 4, and zeros up to the
 * next whole word. */
static void buffer_write(Buffer *buffer, uint64_t offset, const void *data,
                         uint64_t bytes) {
    uint64_t whole = bytes & ~(uint64_t)3;
    queue_write(buffer, offset, data, whole);
    if (bytes != whole) {
        unsigned char last[4] = {0, 0, 0, 0};
        memcpy(last, (const unsigned char *)data + whole, (size_t)(bytes - whole));
        queue_write(buffer, offset + whole, last, 4);
    }
}

static uint64_t now_ns(void) {
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    return (uint64_t)now.tv_sec * 1000000000u + (uint64_t)now.tv_nsec;
}

/* How long a wait polls without blocking before it blocks. */
#define SPIN_NS 50000u

/* Polls the device until a callback sets `*done`. Polls without blocking
 * for up to SPIN_NS, which spares a short wait the sleep and wake-up of a
 * blocking poll, then blocks. The bound changes only how long the wait
 * takes, never what it returns. */
static void device_poll_until(const int *done) {
    uint64_t start = now_ns();
    while (*done == 0 && now_ns() - start < SPIN_NS) {
        wgpuDevicePoll(gpu.device, false, NULL);
    }
    while (*done == 0) wgpuDevicePoll(gpu.device, true, NULL);
}

static void on_work_done(WGPUQueueWorkDoneStatus status, void *done,
                         void *unused) {
    (void)unused;
    *(int *)done = status == WGPUQueueWorkDoneStatus_Success ? 1 : -1;
}

void mn_gpu_device_wait(void) {
    encoder_submit();
    if (!gpu.pending) return;
    int done = 0;
    WGPUQueueWorkDoneCallbackInfo callback = {0};
    callback.mode = WGPUCallbackMode_AllowProcessEvents;
    callback.callback = on_work_done;
    callback.userdata1 = &done;
    wgpuQueueOnSubmittedWorkDone(gpu.queue, callback);
    mn_gpu_waits++;
    device_poll_until(&done);
    if (done < 0) gpu_fail("the device did not finish its work");
    gpu.pending = false;
}

static void on_map(WGPUMapAsyncStatus status, WGPUStringView message,
                   void *done, void *unused) {
    (void)message;
    (void)unused;
    *(int *)done = status == WGPUMapAsyncStatus_Success ? 1 : -1;
}

/* Waits for everything submitted, then returns the first `bytes` bytes of
 * a readback buffer. The copies into it must have been submitted. Unmap
 * the buffer when they have been read. */
static const void *buffer_read(const Buffer *buffer, uint64_t bytes) {
    int done = 0;
    WGPUBufferMapCallbackInfo callback = {0};
    callback.mode = WGPUCallbackMode_AllowProcessEvents;
    callback.callback = on_map;
    callback.userdata1 = &done;
    wgpuBufferMapAsync(buffer->handle, WGPUMapMode_Read, 0, (size_t)bytes,
                       callback);
    mn_gpu_waits++;
    device_poll_until(&done);
    gpu.pending = false;
    if (done < 0) gpu_fail("can't read a buffer back");
    return wgpuBufferGetConstMappedRange(buffer->handle, 0, (size_t)bytes);
}

/* ---- Column copies ------------------------------------------------------- */

static void *column_data(const MnTable *table, const Column *column) {
    if (column->component == MN_GPU_ENTITIES) return table->entities;
    return table->columns[column->component];
}

static const char *component_name(uint32_t component) {
    if (component == MN_GPU_ENTITIES) return "Entity";
    return mn_world.program->components[component].name;
}

static Column *column_find(const MnTable *table, uint32_t component) {
    const struct MnGpuTable *copies = table->gpu;
    if (!copies) return NULL;
    for (uint32_t i = 0; i < copies->count; i++) {
        if (copies->columns[i].component == component) return &copies->columns[i];
    }
    return NULL;
}

/* The GPU copy of a column a block names, created if new. The address is
 * good until the next call for the same table. */
static Column *column_for(MnTable *table, const MnGpuColumn *named) {
    Column *column = column_find(table, named->component);
    if (column) return column;
    struct MnGpuTable *copies = table->gpu;
    if (!copies) {
        copies = mn_calloc(1, sizeof *copies);
        table->gpu = copies;
    }
    if (copies->count == copies->capacity) {
        uint32_t capacity = copies->capacity ? copies->capacity * 2 : 4;
        copies->columns =
            mn_realloc(copies->columns, (size_t)capacity * sizeof(Column));
        copies->capacity = capacity;
    }
    column = &copies->columns[copies->count++];
    memset(column, 0, sizeof *column);
    column->component = named->component;
    column->size = named->size;
    column->where = ON_CPU;
    return column;
}

/* Records which copy of a column is current, and keeps mn_gpu_active true
 * exactly while some component column has a current GPU copy, and
 * mn_gpu_only exactly while some component column's GPU copy is the only
 * current one. */
static void column_set(MnTable *table, Column *column, uint8_t where) {
    uint8_t was = column->where;
    column->where = where;
    if (was == where) return;
    bool counted = column->component != MN_GPU_ENTITIES;
    if ((was != ON_CPU) != (where != ON_CPU)) {
        if (where != ON_CPU) {
            table->gpu->current++;
            if (counted) gpu.current++;
        } else {
            table->gpu->current--;
            if (counted) gpu.current--;
        }
    }
    if (counted && (was == ON_GPU) != (where == ON_GPU)) {
        if (where == ON_GPU) {
            gpu.alone++;
        } else {
            gpu.alone--;
        }
    }
    mn_gpu_active = gpu.current != 0;
    mn_gpu_only = gpu.alone != 0;
}

/* Makes the GPU copy current: copies the column up if only the CPU copy
 * is, into a buffer that has room for the table's rows. */
static void column_upload(MnTable *table, Column *column) {
    if (column->where != ON_CPU) return;
    uint64_t bytes = (uint64_t)table->count * column->size;
    uint64_t need = round4(bytes);
    if (column->buffer.size < need) {
        uint64_t roomy = round4((uint64_t)table->capacity * column->size);
        buffer_make(&column->buffer, roomy <= gpu.max_binding ? roomy : need,
                    COLUMN_USAGE);
    }
    buffer_write(&column->buffer, 0, column_data(table, column), bytes);
    column->version = ++gpu.serial;
    column_set(table, column, ON_BOTH);
    if (column->component != MN_GPU_ENTITIES) mn_gpu_uploads++;
}

/* ---- Round trips --------------------------------------------------------- */

/* Lists a column for the next round trip, when only its GPU copy is
 * current and it is not listed yet. */
static void fetch_column(MnTable *table, Column *column) {
    if (column->where != ON_GPU || column->fetching) return;
    column->fetching = true;
    if (gpu.fetch_count == gpu.fetch_capacity) {
        uint32_t capacity = gpu.fetch_capacity ? gpu.fetch_capacity * 2 : 16;
        gpu.fetches = mn_realloc(gpu.fetches, capacity * sizeof(Fetch));
        gpu.fetch_capacity = capacity;
    }
    Fetch *fetch = &gpu.fetches[gpu.fetch_count++];
    fetch->table = table;
    fetch->component = column->component;
    fetch->offset = 0;
}

/* The bytes a listed column brings back. */
static uint64_t fetch_bytes(const Fetch *fetch, const Column *column) {
    return round4((uint64_t)fetch->table->count * column->size);
}

/* Brings back every column listed since the last round trip: the copies
 * into the readback buffer are recorded in the open encoder, which is
 * submitted, and the program waits once, or as many times as the largest
 * buffer needs. Both copies of those columns are current afterwards. No
 * column may be created between listing and bringing back. */
static void fetch_run(void) {
    uint32_t first = 0;
    while (first < gpu.fetch_count) {
        uint64_t total = 0;
        uint32_t end = first;
        while (end < gpu.fetch_count) {
            Fetch *fetch = &gpu.fetches[end];
            Column *column = column_find(fetch->table, fetch->component);
            uint64_t bytes = (fetch_bytes(fetch, column) + 7) & ~(uint64_t)7;
            if (end > first && total + bytes > gpu.max_buffer) break;
            fetch->offset = total;
            total += bytes;
            end++;
        }
        if (total != 0) {
            if (gpu.readback.size < total) {
                uint64_t roomy = gpu.readback.size * 2;
                if (roomy > gpu.max_buffer) roomy = gpu.max_buffer;
                buffer_make(&gpu.readback, roomy > total ? roomy : total,
                            READBACK_USAGE);
            }
            for (uint32_t i = first; i < end; i++) {
                Fetch *fetch = &gpu.fetches[i];
                Column *column = column_find(fetch->table, fetch->component);
                uint64_t bytes = fetch_bytes(fetch, column);
                if (bytes != 0) {
                    buffer_copy(&column->buffer, 0, &gpu.readback, fetch->offset,
                                bytes);
                }
            }
            encoder_submit();
            const unsigned char *from = buffer_read(&gpu.readback, total);
            for (uint32_t i = first; i < end; i++) {
                Fetch *fetch = &gpu.fetches[i];
                Column *column = column_find(fetch->table, fetch->component);
                memcpy(column_data(fetch->table, column), from + fetch->offset,
                       (size_t)fetch->table->count * column->size);
            }
            wgpuBufferUnmap(gpu.readback.handle);
        }
        for (uint32_t i = first; i < end; i++) {
            Fetch *fetch = &gpu.fetches[i];
            Column *column = column_find(fetch->table, fetch->component);
            column->fetching = false;
            column_set(fetch->table, column, ON_BOTH);
            mn_gpu_downloads++;
        }
        first = end;
    }
    gpu.fetch_count = 0;
}

void mn_gpu_use(MnQuery *query, const MnUse *uses, uint32_t count) {
    for (uint32_t t = 0; t < query->table_count; t++) {
        MnTable *table = query->tables[t];
        if (table->count == 0 || !table->gpu) continue;
        for (uint32_t i = 0; i < count; i++) {
            Column *column = column_find(table, uses[i].component);
            if (column) fetch_column(table, column);
        }
    }
    fetch_run();
}

void mn_gpu_wrote(MnTable *table, uint32_t component) {
    Column *column = column_find(table, component);
    if (column) column_set(table, column, ON_CPU);
}

void mn_gpu_sync(MnTable *table, uint32_t component, bool write) {
    Column *column = column_find(table, component);
    if (!column) return;
    fetch_column(table, column);
    fetch_run();
    if (write) column_set(table, column, ON_CPU);
}

void mn_gpu_reclaim(MnTable *table) {
    struct MnGpuTable *copies = table->gpu;
    if (!copies || copies->current == 0) return;
    for (uint32_t i = 0; i < copies->count; i++) {
        fetch_column(table, &copies->columns[i]);
    }
    fetch_run();
    for (uint32_t i = 0; i < copies->count; i++) {
        column_set(table, &copies->columns[i], ON_CPU);
    }
}

/* ---- Blocks -------------------------------------------------------------- */

/* What a block runs one table of its outer query with. */
typedef struct Target {
    WGPUBindGroup group;
    /* The serial of the buffer at each binding of `group`. */
    uint64_t *serials;
} Target;

/* The snapshot of one inner column, when it is a copy. */
typedef struct Snapshot {
    Buffer buffer;
    /* What the buffer holds: for each table with rows, in the query's
     * order, the table's id, its row count and its column's version. */
    uint64_t *contents;
    uint32_t content_count;
    uint32_t content_capacity;
} Snapshot;

/* What the device keeps for a block. */
typedef struct Block {
    WGPUBindGroupLayout layout;
    WGPUComputePipeline pipeline;
    /* The uniform words, the outer columns, the inner columns and the
     * error word. */
    uint32_t binding_count;
    /* One per table of the outer query, in the query's order. */
    Target *targets;
    uint32_t target_count;
    /* The uniform words of every table, table t's at t * uniform_stride
     * bytes, and the copy of them in CPU memory that a run fills and
     * writes in one piece. */
    Buffer uniforms;
    uint32_t *uniform_words;
    uint32_t uniform_stride;
    /* One per inner column. */
    Snapshot *snapshots;
    uint32_t snapshot_count;
    /* The error word, and where it is copied to be read. */
    Buffer error;
    Buffer error_read;
    /* Working space: the buffer for each binding of the table being run. */
    Buffer *bound;
    WGPUBindGroupEntry *entries;
} Block;

static bool block_writes(const MnGpuBlock *block, uint32_t component) {
    for (uint32_t i = 0; i < block->outer_count; i++) {
        const MnGpuColumn *column = &block->outer_columns[i];
        if (column->component == component && column->write) return true;
    }
    return false;
}

static uint32_t block_storage_buffers(const MnGpuBlock *block) {
    return block->outer_count + block->inner_count + (block->fallible ? 1 : 0);
}

/* The distance in bytes from one table's uniform words to the next's. */
static uint64_t block_uniform_stride(const MnGpuBlock *block) {
    uint64_t bytes = (uint64_t)block->uniform_words * 4;
    return (bytes + gpu.uniform_align - 1) / gpu.uniform_align *
           gpu.uniform_align;
}

/* Why the device cannot run the block as its tables are now, in
 * gpu.reason, or NULL when it can. */
static const char *block_unfit(const MnGpuBlock *block) {
    char *reason = gpu.reason;
    size_t room = sizeof gpu.reason;
    uint32_t storage = block_storage_buffers(block);
    if (storage > gpu.max_storage_buffers) {
        snprintf(reason, room,
                 "it binds %u storage buffers and the device binds at most %u",
                 (unsigned)storage, (unsigned)gpu.max_storage_buffers);
        return reason;
    }
    uint64_t uniform_bytes = (uint64_t)block->uniform_words * 4;
    if (uniform_bytes > gpu.max_uniform) {
        snprintf(reason, room,
                 "its resources and constants take %llu bytes and the device "
                 "binds at most %llu",
                 (unsigned long long)uniform_bytes,
                 (unsigned long long)gpu.max_uniform);
        return reason;
    }
    const MnQuery *query = block->outer;
    if (block_uniform_stride(block) * query->table_count > gpu.max_buffer) {
        snprintf(reason, room,
                 "its query matches %u tables, more than the device has room "
                 "for",
                 (unsigned)query->table_count);
        return reason;
    }
    for (uint32_t t = 0; t < query->table_count; t++) {
        uint64_t rows = query->tables[t]->count;
        if ((rows + 63) / 64 > gpu.max_groups && gpu.max_groups < MAX_GROUPS) {
            snprintf(reason, room,
                     "a table has %llu rows and the device runs at most %llu "
                     "in one dispatch",
                     (unsigned long long)rows,
                     (unsigned long long)gpu.max_groups * 64);
            return reason;
        }
        for (uint32_t i = 0; i < block->outer_count; i++) {
            const MnGpuColumn *column = &block->outer_columns[i];
            uint64_t bytes = round4(rows * column->size);
            if (bytes <= gpu.max_binding) continue;
            snprintf(reason, room,
                     "%s takes %llu bytes in one table and the device binds "
                     "at most %llu",
                     component_name(column->component),
                     (unsigned long long)bytes,
                     (unsigned long long)gpu.max_binding);
            return reason;
        }
    }
    query = block->inner;
    if (!query) return NULL;
    uint64_t rows = 0;
    for (uint32_t t = 0; t < query->table_count; t++) {
        rows += query->tables[t]->count;
    }
    if (rows > UINT32_MAX) mn_out_of_memory();
    for (uint32_t i = 0; i < block->inner_count; i++) {
        const MnGpuColumn *column = &block->inner_columns[i];
        uint64_t bytes = round4(rows * column->size);
        if (bytes <= gpu.max_binding) continue;
        snprintf(reason, room,
                 "its nested query reads %llu bytes of %s and the device "
                 "binds at most %llu",
                 (unsigned long long)bytes, component_name(column->component),
                 (unsigned long long)gpu.max_binding);
        return reason;
    }
    return NULL;
}

/* Makes the CPU copy of every column the block uses current, with one
 * round trip, and the only current one for those it writes. */
static void block_to_cpu(const MnGpuBlock *block) {
    MnQuery *query = block->outer;
    for (uint32_t t = 0; t < query->table_count; t++) {
        MnTable *table = query->tables[t];
        if (table->count == 0) continue;
        for (uint32_t i = 0; i < block->outer_count; i++) {
            Column *column = column_find(table, block->outer_columns[i].component);
            if (column) fetch_column(table, column);
        }
    }
    if (block->inner) {
        query = block->inner;
        for (uint32_t t = 0; t < query->table_count; t++) {
            MnTable *table = query->tables[t];
            if (table->count == 0) continue;
            for (uint32_t i = 0; i < block->inner_count; i++) {
                Column *column =
                    column_find(table, block->inner_columns[i].component);
                if (column) fetch_column(table, column);
            }
        }
    }
    fetch_run();
    query = block->outer;
    for (uint32_t t = 0; t < query->table_count; t++) {
        MnTable *table = query->tables[t];
        if (table->count == 0) continue;
        for (uint32_t i = 0; i < block->outer_count; i++) {
            const MnGpuColumn *named = &block->outer_columns[i];
            Column *column = column_find(table, named->component);
            if (column && named->write) column_set(table, column, ON_CPU);
        }
    }
}


/* Builds the block's pipeline. Its bind group layout is written out from
 * the block's description, in the order midnight_rt.h gives. */
static Block *block_make(const MnGpuBlock *block) {
    Block *state = mn_calloc(1, sizeof *state);
    uint32_t count = 1 + block_storage_buffers(block);
    state->binding_count = count;
    state->bound = mn_calloc(count, sizeof(Buffer));
    state->entries = mn_calloc(count, sizeof(WGPUBindGroupEntry));
    state->snapshot_count = block->inner_count;
    state->snapshots = mn_calloc(block->inner_count, sizeof(Snapshot));
    state->uniform_stride = (uint32_t)block_uniform_stride(block);

    /* What the device calls the block's objects in its error messages. */
    char name[128];
    snprintf(name, sizeof name, "%s (each %u)", block->system,
             (unsigned)block->each);

    WGPUBindGroupLayoutEntry *entries =
        mn_calloc(count, sizeof(WGPUBindGroupLayoutEntry));
    for (uint32_t i = 0; i < count; i++) {
        entries[i].binding = i;
        entries[i].visibility = WGPUShaderStage_Compute;
        entries[i].buffer.type = WGPUBufferBindingType_ReadOnlyStorage;
    }
    entries[0].buffer.type = WGPUBufferBindingType_Uniform;
    for (uint32_t i = 0; i < block->outer_count; i++) {
        if (block->outer_columns[i].write) {
            entries[1 + i].buffer.type = WGPUBufferBindingType_Storage;
        }
    }
    if (block->fallible) {
        entries[count - 1].buffer.type = WGPUBufferBindingType_Storage;
    }
    WGPUBindGroupLayoutDescriptor layout = {0};
    layout.label = view(name);
    layout.entryCount = count;
    layout.entries = entries;
    state->layout = wgpuDeviceCreateBindGroupLayout(gpu.device, &layout);
    free(entries);

    WGPUPipelineLayoutDescriptor pipeline_layout = {0};
    pipeline_layout.label = view(name);
    pipeline_layout.bindGroupLayoutCount = 1;
    pipeline_layout.bindGroupLayouts = &state->layout;
    WGPUPipelineLayout bindings =
        wgpuDeviceCreatePipelineLayout(gpu.device, &pipeline_layout);

    WGPUShaderSourceWGSL source = {0};
    source.chain.sType = WGPUSType_ShaderSourceWGSL;
    source.code = view(block->wgsl);
    WGPUShaderModuleDescriptor module = {0};
    module.nextInChain = &source.chain;
    module.label = view(name);
    WGPUShaderModule shader = wgpuDeviceCreateShaderModule(gpu.device, &module);

    WGPUComputePipelineDescriptor pipeline = {0};
    pipeline.label = view(name);
    pipeline.layout = bindings;
    pipeline.compute.module = shader;
    pipeline.compute.entryPoint = view("main");
    state->pipeline = wgpuDeviceCreateComputePipeline(gpu.device, &pipeline);
    if (!state->layout || !bindings || !shader || !state->pipeline) {
        gpu_fail("can't build the pipeline of %s (each %u)", block->system,
                 (unsigned)block->each);
    }
    wgpuShaderModuleRelease(shader);
    wgpuPipelineLayoutRelease(bindings);

    if (block->fallible) {
        buffer_make(&state->error, 4, COLUMN_USAGE);
        buffer_make(&state->error_read, 4, READBACK_USAGE);
    }
    return state;
}

void mn_gpu_device_forget(void *block_state) {
    Block *state = block_state;
    if (!state) return;
    for (uint32_t t = 0; t < state->target_count; t++) {
        Target *target = &state->targets[t];
        if (target->group) wgpuBindGroupRelease(target->group);
        free(target->serials);
    }
    buffer_drop(&state->uniforms);
    free(state->uniform_words);
    for (uint32_t i = 0; i < state->snapshot_count; i++) {
        buffer_drop(&state->snapshots[i].buffer);
        free(state->snapshots[i].contents);
    }
    buffer_drop(&state->error);
    buffer_drop(&state->error_read);
    wgpuComputePipelineRelease(state->pipeline);
    wgpuBindGroupLayoutRelease(state->layout);
    free(state->targets);
    free(state->snapshots);
    free(state->bound);
    free(state->entries);
    free(state);
}

/* Makes the snapshot of inner column `i` ready in GPU memory and returns
 * the buffer that holds it. A column the block does not write, in the only
 * table with rows, stands for itself. Anything else is a copy of each
 * table's column, one after another, made again only when a column's
 * contents or the rows have changed: by copies in GPU memory when every
 * table's bytes end at a word, and from CPU memory otherwise. */
static Buffer *snapshot_ready(const MnGpuBlock *block, Block *state,
                              uint32_t i, uint64_t rows, MnTable *only) {
    const MnGpuColumn *named = &block->inner_columns[i];
    MnQuery *query = block->inner;
    if (only && !block_writes(block, named->component)) {
        Column *column = column_for(only, named);
        column_upload(only, column);
        return &column->buffer;
    }

    Snapshot *snapshot = &state->snapshots[i];
    uint64_t bytes = rows * named->size;
    uint64_t need = bytes == 0 ? 4 : round4(bytes);
    if (snapshot->buffer.size < need) {
        uint64_t roomy = snapshot->buffer.size * 2;
        if (roomy > gpu.max_binding) roomy = gpu.max_binding;
        buffer_make(&snapshot->buffer, roomy > need ? roomy : need,
                    SNAPSHOT_USAGE);
        snapshot->content_count = 0;
    }

    if (gpu.contents_capacity < query->table_count) {
        gpu.contents = mn_realloc(
            gpu.contents, (size_t)query->table_count * 3 * sizeof(uint64_t));
        gpu.contents_capacity = query->table_count;
    }
    uint32_t count = 0;
    for (uint32_t t = 0; t < query->table_count; t++) {
        MnTable *table = query->tables[t];
        if (table->count == 0) continue;
        Column *column = column_for(table, named);
        column_upload(table, column);
        gpu.contents[count++] = table->id;
        gpu.contents[count++] = table->count;
        gpu.contents[count++] = column->version;
    }
    if (count == snapshot->content_count &&
        (count == 0 || memcmp(gpu.contents, snapshot->contents,
                              count * sizeof(uint64_t)) == 0)) {
        return &snapshot->buffer;
    }
    if (snapshot->content_capacity < count) {
        snapshot->contents =
            mn_realloc(snapshot->contents, count * sizeof(uint64_t));
        snapshot->content_capacity = count;
    }
    if (count != 0) {
        memcpy(snapshot->contents, gpu.contents, count * sizeof(uint64_t));
    }
    snapshot->content_count = count;

    if (named->size % 4 == 0) {
        uint64_t offset = 0;
        for (uint32_t t = 0; t < query->table_count; t++) {
            MnTable *table = query->tables[t];
            if (table->count == 0) continue;
            uint64_t part = (uint64_t)table->count * named->size;
            buffer_copy(&column_for(table, named)->buffer, 0, &snapshot->buffer,
                        offset, part);
            offset += part;
        }
        return &snapshot->buffer;
    }

    if (gpu.bytes_capacity < need) {
        gpu.bytes = mn_realloc(gpu.bytes, (size_t)need);
        gpu.bytes_capacity = need;
    }
    unsigned char *out = gpu.bytes;
    for (uint32_t t = 0; t < query->table_count; t++) {
        MnTable *table = query->tables[t];
        if (table->count == 0) continue;
        Column *column = column_for(table, named);
        if (column->where == ON_GPU) {
            fetch_column(table, column);
            fetch_run();
        }
        size_t part = (size_t)table->count * named->size;
        memcpy(out, column_data(table, column), part);
        out += part;
    }
    if (bytes != 0) buffer_write(&snapshot->buffer, 0, gpu.bytes, bytes);
    return &snapshot->buffer;
}

/* Points the bind group of the block's table `t` at the buffers in
 * state->bound, building it again when any of them is not the one it was
 * built from. The uniform words are the table's part of their buffer. */
static void target_bind(const MnGpuBlock *block, Block *state, uint32_t t) {
    Target *target = &state->targets[t];
    uint32_t count = state->binding_count;
    if (!target->serials) target->serials = mn_calloc(count, sizeof(uint64_t));
    bool same = target->group != NULL;
    for (uint32_t i = 0; i < count; i++) {
        if (target->serials[i] != state->bound[i].serial) same = false;
        target->serials[i] = state->bound[i].serial;
    }
    if (same) return;
    if (target->group) wgpuBindGroupRelease(target->group);
    memset(state->entries, 0, count * sizeof(WGPUBindGroupEntry));
    for (uint32_t i = 0; i < count; i++) {
        state->entries[i].binding = i;
        state->entries[i].buffer = state->bound[i].handle;
        state->entries[i].size = state->bound[i].size;
    }
    state->entries[0].offset = (uint64_t)t * state->uniform_stride;
    state->entries[0].size = (uint64_t)block->uniform_words * 4;
    WGPUBindGroupDescriptor group = {0};
    group.layout = state->layout;
    group.entryCount = count;
    group.entries = state->entries;
    target->group = wgpuDeviceCreateBindGroup(gpu.device, &group);
    if (!target->group) gpu_fail("can't bind the buffers of a block");
}

const char *mn_gpu_device_run(MnGpuBlock *block, uint32_t *uniform,
                              void **block_state) {
    if (!gpu.device) return "no device";
    const char *reason = block_unfit(block);
    if (reason) {
        block_to_cpu(block);
        return reason;
    }
    MnQuery *outer = block->outer;
    uint32_t populated_outer = 0;
    for (uint32_t t = 0; t < outer->table_count; t++) {
        if (outer->tables[t]->count != 0) populated_outer++;
    }
    if (populated_outer == 0) return NULL;

    Block *state = *block_state;
    if (!state) {
        state = block_make(block);
        *block_state = state;
    }
    Buffer *bound = state->bound;

    uint64_t rows = 0;
    MnTable *only = NULL;
    if (block->inner) {
        MnQuery *inner = block->inner;
        uint32_t populated = 0;
        for (uint32_t t = 0; t < inner->table_count; t++) {
            MnTable *table = inner->tables[t];
            if (table->count == 0) continue;
            rows += table->count;
            populated++;
            only = table;
        }
        if (populated != 1) only = NULL;
        for (uint32_t i = 0; i < block->inner_count; i++) {
            bound[1 + block->outer_count + i] =
                *snapshot_ready(block, state, i, rows, only);
        }
    }
    uniform[1] = (uint32_t)rows;
    if (block->fallible) {
        uint32_t none = UINT32_MAX;
        queue_write(&state->error, 0, &none, 4);
        bound[state->binding_count - 1] = state->error;
    }

    if (state->target_count < outer->table_count) {
        state->targets = mn_realloc(
            state->targets, (size_t)outer->table_count * sizeof(Target));
        memset(state->targets + state->target_count, 0,
               (outer->table_count - state->target_count) * sizeof(Target));
        state->target_count = outer->table_count;
    }
    size_t stride = state->uniform_stride;
    if (state->uniforms.size < stride * outer->table_count) {
        /* Room for as many tables again, when the device has it. */
        uint64_t size = stride * outer->table_count * 2;
        if (size > gpu.max_buffer) size = stride * outer->table_count;
        buffer_make(&state->uniforms, size, UNIFORM_USAGE);
        free(state->uniform_words);
        state->uniform_words = mn_calloc((size_t)size, 1);
    }
    bound[0] = state->uniforms;

    /* Every table's columns and uniform words are made ready before the
     * dispatches are recorded. */
    size_t uniform_end = 0;
    for (uint32_t t = 0; t < outer->table_count; t++) {
        MnTable *table = outer->tables[t];
        if (table->count == 0) continue;
        for (uint32_t i = 0; i < block->outer_count; i++) {
            Column *column = column_for(table, &block->outer_columns[i]);
            column_upload(table, column);
            bound[1 + i] = column->buffer;
        }
        target_bind(block, state, t);
        uniform[0] = table->count;
        memcpy((char *)state->uniform_words + t * stride, uniform,
               (size_t)block->uniform_words * 4);
        uniform_end = (t + 1) * stride;
    }
    queue_write(&state->uniforms, 0, state->uniform_words, uniform_end);

    WGPUComputePassEncoder pass = pass_open();
    wgpuComputePassEncoderSetPipeline(pass, state->pipeline);
    buffer_use(&state->uniforms);
    if (block->fallible) buffer_use(&state->error);
    for (uint32_t i = 0; i < block->inner_count; i++) {
        const MnGpuColumn *named = &block->inner_columns[i];
        if (only && !block_writes(block, named->component)) {
            buffer_use(&column_find(only, named->component)->buffer);
        } else {
            buffer_use(&state->snapshots[i].buffer);
        }
    }
    for (uint32_t t = 0; t < outer->table_count; t++) {
        MnTable *table = outer->tables[t];
        if (table->count == 0) continue;
        /* Row r is run by invocation (r % MN_GPU_ROW_SPAN, r /
         * MN_GPU_ROW_SPAN). */
        uint64_t groups = ((uint64_t)table->count + 63) / 64;
        uint32_t across = groups < MAX_GROUPS ? (uint32_t)groups : MAX_GROUPS;
        uint32_t down = (uint32_t)((groups + MAX_GROUPS - 1) / MAX_GROUPS);
        wgpuComputePassEncoderSetBindGroup(pass, 0, state->targets[t].group, 0,
                                           NULL);
        wgpuComputePassEncoderDispatchWorkgroups(pass, across, down, 1);

        for (uint32_t i = 0; i < block->outer_count; i++) {
            Column *column = column_find(table, block->outer_columns[i].component);
            buffer_use(&column->buffer);
            if (!block->outer_columns[i].write) continue;
            column->version = ++gpu.serial;
            column_set(table, column, ON_GPU);
        }
    }

    if (block->fallible) {
        buffer_copy(&state->error, 0, &state->error_read, 0, 4);
        encoder_submit();
        uint32_t lowest;
        memcpy(&lowest, buffer_read(&state->error_read, 4), 4);
        wgpuBufferUnmap(state->error_read.handle);
        if (lowest != UINT32_MAX) {
            const MnGpuSite *site = &block->sites[lowest];
            mn_panic(site->loc, "%s", site->message);
        }
    }
    return NULL;
}

/* ---- The device ---------------------------------------------------------- */

static void on_adapter(WGPURequestAdapterStatus status, WGPUAdapter adapter,
                       WGPUStringView message, void *out, void *unused) {
    (void)message;
    (void)unused;
    if (status == WGPURequestAdapterStatus_Success) *(WGPUAdapter *)out = adapter;
}

static void on_device(WGPURequestDeviceStatus status, WGPUDevice device,
                      WGPUStringView message, void *out, void *unused) {
    (void)message;
    (void)unused;
    if (status == WGPURequestDeviceStatus_Success) *(WGPUDevice *)out = device;
}

static const char *backend_name(WGPUBackendType backend) {
    switch (backend) {
    case WGPUBackendType_Vulkan:
        return "Vulkan";
    case WGPUBackendType_Metal:
        return "Metal";
    case WGPUBackendType_D3D12:
        return "Direct3D 12";
    default:
        return "another backend";
    }
}

const char *mn_gpu_device_start(bool software) {
    WGPUInstanceExtras extras = {0};
    extras.chain.sType = (WGPUSType)WGPUSType_InstanceExtras;
    extras.backends = WGPUInstanceBackend_Primary;
    WGPUInstanceDescriptor instance = {0};
    instance.nextInChain = &extras.chain;
    gpu.instance = wgpuCreateInstance(&instance);
    if (!gpu.instance) return NULL;

    WGPUAdapter adapter = NULL;
    WGPURequestAdapterOptions options = {0};
    options.featureLevel = WGPUFeatureLevel_Core;
    options.powerPreference = WGPUPowerPreference_HighPerformance;
    WGPURequestAdapterCallbackInfo adapter_callback = {0};
    adapter_callback.mode = WGPUCallbackMode_AllowSpontaneous;
    adapter_callback.callback = on_adapter;
    adapter_callback.userdata1 = &adapter;
    wgpuInstanceRequestAdapter(gpu.instance, &options, adapter_callback);
    wgpuInstanceProcessEvents(gpu.instance);

    WGPULimits limits = {0};
    if (adapter) {
        WGPUAdapterInfo info = {0};
        wgpuAdapterGetInfo(adapter, &info);
        snprintf(gpu.name, sizeof gpu.name, "%.*s (%s)",
                 view_length(info.device), info.device.data ? info.device.data : "",
                 backend_name(info.backendType));
        bool usable = info.adapterType != WGPUAdapterType_CPU || software;
        wgpuAdapterInfoFreeMembers(info);
        if (usable && wgpuAdapterGetLimits(adapter, &limits) == WGPUStatus_Success) {
            WGPUDeviceDescriptor device = {0};
            device.requiredLimits = &limits;
            device.deviceLostCallbackInfo.mode = WGPUCallbackMode_AllowSpontaneous;
            device.deviceLostCallbackInfo.callback = on_lost;
            device.uncapturedErrorCallbackInfo.callback = on_error;
            WGPURequestDeviceCallbackInfo device_callback = {0};
            device_callback.mode = WGPUCallbackMode_AllowSpontaneous;
            device_callback.callback = on_device;
            device_callback.userdata1 = &gpu.device;
            wgpuAdapterRequestDevice(adapter, &device, device_callback);
            wgpuInstanceProcessEvents(gpu.instance);
        }
        wgpuAdapterRelease(adapter);
    }
    if (!gpu.device) {
        wgpuInstanceRelease(gpu.instance);
        gpu.instance = NULL;
        return NULL;
    }
    gpu.queue = wgpuDeviceGetQueue(gpu.device);
    gpu.max_buffer = limits.maxBufferSize;
    gpu.max_binding = limits.maxStorageBufferBindingSize < limits.maxBufferSize
                          ? limits.maxStorageBufferBindingSize
                          : limits.maxBufferSize;
    gpu.max_binding &= ~(uint64_t)3;
    gpu.max_uniform = limits.maxUniformBufferBindingSize;
    gpu.uniform_align = limits.minUniformBufferOffsetAlignment;
    if (gpu.uniform_align < 4) gpu.uniform_align = 4;
    gpu.max_storage_buffers = limits.maxStorageBuffersPerShaderStage;
    if (gpu.max_storage_buffers >= limits.maxBindingsPerBindGroup) {
        gpu.max_storage_buffers = limits.maxBindingsPerBindGroup - 1;
    }
    gpu.max_groups = limits.maxComputeWorkgroupsPerDimension;
    return gpu.name;
}

void mn_gpu_device_finish(void) {
    if (!gpu.device) return;
    mn_gpu_device_wait();
    for (uint32_t t = 0; t < mn_world.table_count; t++) {
        MnTable *table = mn_world.tables[t];
        struct MnGpuTable *copies = table->gpu;
        if (!copies) continue;
        for (uint32_t i = 0; i < copies->count; i++) {
            buffer_drop(&copies->columns[i].buffer);
        }
        free(copies->columns);
        free(copies);
        table->gpu = NULL;
    }
    buffer_drop(&gpu.readback);
    free(gpu.bytes);
    free(gpu.contents);
    free(gpu.fetches);
    gpu.bytes = NULL;
    gpu.contents = NULL;
    gpu.fetches = NULL;
    gpu.current = 0;
    gpu.alone = 0;
    mn_gpu_active = false;
    mn_gpu_only = false;

    gpu.finishing = true;
    wgpuQueueRelease(gpu.queue);
    wgpuDeviceRelease(gpu.device);
    wgpuInstanceRelease(gpu.instance);
    gpu.queue = NULL;
    gpu.device = NULL;
    gpu.instance = NULL;
}
