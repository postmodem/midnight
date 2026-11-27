/*
 * The Midnight runtime. See midnight_rt.h for what each function promises.
 */

#define _POSIX_C_SOURCE 200809L

#include "midnight_rt.h"

/* MN_GPU_SYSTEMS is defined when the program declares a GPU system, and
 * MN_GPU when midnight_gpu.c is compiled with the program: for its GPU
 * systems, its window, or both. MN_WINDOW is defined when the program has
 * a window, and MN_FRAMES when midnight_window.c is compiled with it,
 * which also means MN_GPU. */
#ifdef MN_GPU
#include "midnight_gpu.h"
#endif
#ifdef MN_FRAMES
#include "midnight_window.h"
#endif

/* MN_THREADS is defined when the program has a loop that can be split, and
 * midnight_pool.c is then compiled with it. */
#ifdef MN_THREADS
#include "midnight_pool.h"

#include <pthread.h>
#include <setjmp.h>
#endif

#include <errno.h>
#include <math.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

#define MN_NONE UINT32_MAX

MnWorld mn_world;
MnTime mn_time = {0, 1.0 / 60, 0.0};

/* ---- Leaving the program ----------------------------------------------- */

/* mn_leave, mn_out_of_memory, mn_malloc, mn_calloc and mn_realloc are also
 * called from midnight_gpu.c, which finds them in midnight_gpu.h. */

/* Flushes both standard streams and ends the process at once. */
_Noreturn void mn_leave(int status) {
    fflush(stdout);
    fflush(stderr);
    _Exit(status);
}

_Noreturn void mn_out_of_memory(void);

#ifdef MN_THREADS
_Thread_local struct MnLoopThread *mn_loop_thread;

/* Keeps the error of the piece the thread is running, when no earlier
 * piece has failed, and leaves the piece. `text` is the whole line, which
 * it takes over. */
static _Noreturn void piece_fail(struct MnLoopThread *thread, char *text);

/* Makes room for `need` bytes of what the thread's piece queues (`kind`
 * 0), emits (`kind` 1 + the event's id) or draws (`kind` 1 + the number of
 * events), counts one record more, and returns where the bytes start. */
static unsigned char *piece_bytes(struct MnLoopThread *thread, uint32_t kind,
                                  size_t need);
#endif

_Noreturn void mn_panic(MnLoc loc, const char *format, ...) {
    va_list args;
#ifdef MN_THREADS
    /* In a piece the error is kept, and reported if no earlier row
     * fails. */
    struct MnLoopThread *thread = mn_loop_thread;
    if (thread) {
        va_start(args, format);
        int said = vsnprintf(NULL, 0, format, args);
        va_end(args);
        int head = snprintf(NULL, 0, "%s:%d: runtime error: ", loc.file,
                            (int)loc.line);
        size_t size = (size_t)(head < 0 ? 0 : head) +
                      (size_t)(said < 0 ? 0 : said) + 2;
        char *text = malloc(size);
        if (!text) mn_out_of_memory();
        int at = snprintf(text, size, "%s:%d: runtime error: ", loc.file,
                          (int)loc.line);
        va_start(args, format);
        at += vsnprintf(text + at, size - (size_t)at, format, args);
        va_end(args);
        text[at] = '\n';
        text[at + 1] = '\0';
        piece_fail(thread, text);
    }
#endif
    fflush(stdout);
    fprintf(stderr, "%s:%d: runtime error: ", loc.file, (int)loc.line);
    va_start(args, format);
    vfprintf(stderr, format, args);
    va_end(args);
    fputc('\n', stderr);
    mn_leave(1);
}

static void gpu_stats_line(void);

#ifdef MN_WINDOW
static void window_finish(void);
#endif

_Noreturn void mn_exit(int64_t status) {
#ifdef MN_WINDOW
    window_finish();
#endif
    gpu_stats_line();
    mn_leave((int)(status & 0xff));
}

/* ---- Memory -------------------------------------------------------------- */

_Noreturn void mn_out_of_memory(void) {
    fflush(stdout);
    fputs("midnight: out of memory\n", stderr);
    abort();
}

void *mn_malloc(size_t size) {
    void *memory = malloc(size ? size : 1);
    if (!memory) mn_out_of_memory();
    return memory;
}

void *mn_calloc(size_t count, size_t size) {
    void *memory = calloc(count ? count : 1, size ? size : 1);
    if (!memory) mn_out_of_memory();
    return memory;
}

void *mn_realloc(void *memory, size_t size) {
    memory = realloc(memory, size ? size : 1);
    if (!memory) mn_out_of_memory();
    return memory;
}

/* a * b, or an out-of-memory exit if the product does not fit. */
static size_t mn_mul(size_t a, size_t b) {
    if (b != 0 && a > SIZE_MAX / b) mn_out_of_memory();
    return a * b;
}

/* The number of live string and list buffers. */
#ifdef MN_DEBUG
static int64_t mn_live_buffers;
#define MN_BUFFER_BORN() ((void)mn_live_buffers++)
#define MN_BUFFER_GONE() ((void)mn_live_buffers--)
#else
#define MN_BUFFER_BORN() ((void)0)
#define MN_BUFFER_GONE() ((void)0)
#endif

/* ---- Entity slots -------------------------------------------------------- */

/* Takes a slot off the free list, or a new one, and returns its handle.
 * The entity is alive and in no table. */
static MnEntity slot_take(void) {
    MnWorld *w = &mn_world;
    uint32_t index = w->free_head;
    if (index != MN_NONE) {
        w->free_head = w->slots[index].row;
    } else {
        if (w->slot_count == w->slot_capacity) {
            if (w->slot_capacity > UINT32_MAX / 2) mn_out_of_memory();
            uint32_t capacity = w->slot_capacity ? w->slot_capacity * 2 : 64;
            w->slots = mn_realloc(w->slots, (size_t)capacity * sizeof(MnSlot));
            w->slot_capacity = capacity;
        }
        index = w->slot_count++;
        w->slots[index].generation = 1;
    }
    MnSlot *slot = &w->slots[index];
    slot->table = MN_NO_TABLE;
    slot->row = 0;
    return (MnEntity){index, slot->generation};
}

/* Ends the life of every handle to this slot and puts it on the free
 * list. */
static void slot_free(uint32_t index) {
    MnWorld *w = &mn_world;
    MnSlot *slot = &w->slots[index];
    if (++slot->generation == 0) slot->generation = 1;
    slot->table = MN_NO_TABLE;
    slot->row = w->free_head;
    w->free_head = index;
}

/* ---- Tables -------------------------------------------------------------- */

/* What `columns` holds for a tag, and for a component while the table has
 * no capacity. Never read or written. */
static _Alignas(16) char mn_no_storage[16];

static uint64_t set_hash(const uint32_t *set, uint32_t count) {
    uint64_t hash = 0xcbf29ce484222325u;
    for (uint32_t i = 0; i < count; i++) {
        hash ^= set[i];
        hash *= 0x100000001b3u;
    }
    hash ^= hash >> 29;
    hash *= 0xbf58476d1ce4e5b9u;
    hash ^= hash >> 32;
    return hash;
}

static void table_map_insert(uint32_t id, uint64_t hash) {
    MnWorld *w = &mn_world;
    uint32_t mask = w->table_map_capacity - 1;
    uint32_t at = (uint32_t)hash & mask;
    while (w->table_map[at] != 0) at = (at + 1) & mask;
    w->table_map[at] = id + 1;
}

/* Creates the table for a component set (ascending ids) and registers
 * it. */
static uint32_t table_create(const uint32_t *set, uint32_t set_count,
                             uint64_t hash) {
    MnWorld *w = &mn_world;
    const MnComponentInfo *info = w->program->components;
    size_t component_count = w->program->component_count;

    /* One block: the table, its columns, then its six id arrays. */
    size_t ints = (size_t)set_count * 4 + component_count * 2;
    char *block = mn_malloc(sizeof(MnTable) +
                            component_count * sizeof(void *) +
                            ints * sizeof(uint32_t));
    MnTable *table = (MnTable *)(void *)block;
    table->columns = (void **)(void *)(block + sizeof(MnTable));
    uint32_t *next = (uint32_t *)(void *)(table->columns + component_count);
    table->set = next;
    next += set_count;
    table->data = next;
    next += set_count;
    table->data_sizes = next;
    next += set_count;
    table->drops = next;
    next += set_count;
    table->add_edges = next;
    next += component_count;
    table->remove_edges = next;

    table->entities = NULL;
    table->count = 0;
    table->capacity = 0;
    table->id = w->table_count;
    table->set_count = set_count;
    table->data_count = 0;
    table->drop_count = 0;
    table->hash = hash;
    table->gpu = NULL;
    for (size_t c = 0; c < component_count; c++) {
        table->columns[c] = NULL;
        table->add_edges[c] = MN_NONE;
        table->remove_edges[c] = table->id;
    }
    for (uint32_t i = 0; i < set_count; i++) {
        uint32_t c = set[i];
        table->set[i] = c;
        table->columns[c] = mn_no_storage;
        table->add_edges[c] = table->id;
        table->remove_edges[c] = MN_NONE;
        if (info[c].size != 0) {
            table->data[table->data_count] = c;
            table->data_sizes[table->data_count] = info[c].size;
            table->data_count++;
            if (info[c].drop) table->drops[table->drop_count++] = c;
        }
    }

    if (w->table_count == w->table_capacity) {
        if (w->table_capacity > UINT32_MAX / 4) mn_out_of_memory();
        uint32_t capacity = w->table_capacity ? w->table_capacity * 2 : 16;
        w->tables = mn_realloc(w->tables, (size_t)capacity * sizeof(MnTable *));
        w->table_capacity = capacity;
    }
    w->tables[w->table_count++] = table;

    if (w->table_count > w->table_map_capacity / 2) {
        free(w->table_map);
        w->table_map_capacity *= 2;
        w->table_map = mn_calloc(w->table_map_capacity, sizeof(uint32_t));
        for (uint32_t i = 0; i < w->table_count; i++) {
            table_map_insert(i, w->tables[i]->hash);
        }
    } else {
        table_map_insert(table->id, hash);
    }
    return table->id;
}

/* The table for a component set (ascending ids), created if new. */
static uint32_t table_for_set(const uint32_t *set, uint32_t set_count) {
    MnWorld *w = &mn_world;
    uint64_t hash = set_hash(set, set_count);
    uint32_t mask = w->table_map_capacity - 1;
    for (uint32_t at = (uint32_t)hash & mask;; at = (at + 1) & mask) {
        uint32_t entry = w->table_map[at];
        if (entry == 0) break;
        const MnTable *table = w->tables[entry - 1];
        if (table->hash != hash || table->set_count != set_count) continue;
        uint32_t i = 0;
        while (i < set_count && table->set[i] == set[i]) i++;
        if (i == set_count) return entry - 1;
    }
    return table_create(set, set_count, hash);
}

/* The table reached from `table` by adding one component. */
static uint32_t table_with_one(MnTable *table, uint32_t component) {
    MnWorld *w = &mn_world;
    uint32_t to = table->add_edges[component];
    if (to != MN_NONE) return to;
    uint32_t *set = w->scratch_set;
    uint32_t count = 0;
    uint32_t i = 0;
    while (i < table->set_count && table->set[i] < component) {
        set[count++] = table->set[i++];
    }
    set[count++] = component;
    while (i < table->set_count) set[count++] = table->set[i++];
    to = table_for_set(set, count);
    table->add_edges[component] = to;
    w->tables[to]->remove_edges[component] = table->id;
    return to;
}

/* The table reached from `table` by adding every listed component. */
static uint32_t table_with(MnTable *table, const uint32_t *components,
                           uint32_t count) {
    if (count == 1) return table_with_one(table, components[0]);
    uint32_t *set = mn_world.scratch_set;
    uint32_t set_count = table->set_count;
    if (set_count != 0) memcpy(set, table->set, set_count * sizeof(uint32_t));
    for (uint32_t i = 0; i < count; i++) {
        uint32_t c = components[i];
        if (table->columns[c]) continue;
        uint32_t at = set_count++;
        while (at > 0 && set[at - 1] > c) {
            set[at] = set[at - 1];
            at--;
        }
        set[at] = c;
    }
    if (set_count == table->set_count) return table->id;
    return table_for_set(set, set_count);
}

/* The table reached from `table` by removing every listed component. */
static uint32_t table_without(MnTable *table, const uint32_t *components,
                              uint32_t count) {
    MnWorld *w = &mn_world;
    if (count == 1) {
        uint32_t to = table->remove_edges[components[0]];
        if (to != MN_NONE) return to;
    }
    uint32_t *set = w->scratch_set;
    uint32_t set_count = 0;
    for (uint32_t i = 0; i < table->set_count; i++) {
        uint32_t c = table->set[i];
        uint32_t j = 0;
        while (j < count && components[j] != c) j++;
        if (j == count) set[set_count++] = c;
    }
    if (set_count == table->set_count) return table->id;
    uint32_t to = table_for_set(set, set_count);
    if (count == 1) {
        table->remove_edges[components[0]] = to;
        w->tables[to]->add_edges[components[0]] = table->id;
    }
    return to;
}

/* The table a bundle leads to from `from`, remembered in the bundle. */
static uint32_t bundle_table(MnTable *from, MnBundle *bundle) {
    if (bundle->cached && bundle->cached_from == from->id) {
        return bundle->cached_to;
    }
    uint32_t to = table_with(from, bundle->components, bundle->count);
    bundle->cached_from = from->id;
    bundle->cached_to = to;
    bundle->cached = true;
    return to;
}

static void table_grow(MnTable *table) {
    if (table->capacity > UINT32_MAX / 2) mn_out_of_memory();
    uint32_t capacity = table->capacity ? table->capacity * 2 : 8;
    table->entities =
        mn_realloc(table->entities, (size_t)capacity * sizeof(MnEntity));
    for (uint32_t i = 0; i < table->data_count; i++) {
        uint32_t c = table->data[i];
        void *old = table->capacity ? table->columns[c] : NULL;
        table->columns[c] =
            mn_realloc(old, mn_mul(capacity, table->data_sizes[i]));
    }
    table->capacity = capacity;
}

/* Call before a row moves from `table` to another table: makes CPU memory
 * have the row's value in every column but those in `skip`, which the
 * move leaves behind. */
static inline void table_moving(MnTable *table, uint32_t row,
                                const uint32_t *skip, uint32_t skip_count) {
#ifdef MN_GPU
    if (table->gpu && mn_gpu_only) {
        mn_gpu_want_row(table, row, skip, skip_count);
        mn_gpu_fetch();
    }
#else
    (void)table;
    (void)row;
    (void)skip;
    (void)skip_count;
#endif
}

/* Call before the value of a row is replaced in place. */
static inline void table_replacing(MnTable *table, uint32_t row,
                                   uint32_t component) {
#ifdef MN_GPU
    if (table->gpu && mn_gpu_active) {
        mn_gpu_row(table, row, component, MN_ROW_WHOLE);
    }
#else
    (void)table;
    (void)row;
    (void)component;
#endif
}

/* Adds a row for `e` and returns its index. The row's values are not
 * initialised. */
static inline uint32_t table_append(MnTable *table, MnEntity e) {
    if (table->count == table->capacity) table_grow(table);
    uint32_t row = table->count++;
    table->entities[row] = e;
    return row;
}

/* Takes a row out by moving the last row into its place. The values of
 * the row are not dropped. */
static void table_remove(MnTable *table, uint32_t row) {
#ifdef MN_GPU
    if (table->gpu && mn_gpu_active) mn_gpu_leave(table, row);
#endif
    uint32_t last = --table->count;
    if (row == last) return;
    for (uint32_t i = 0; i < table->data_count; i++) {
        size_t size = table->data_sizes[i];
        char *column = table->columns[table->data[i]];
        memcpy(column + row * size, column + last * size, size);
    }
    MnEntity moved = table->entities[last];
    table->entities[row] = moved;
    mn_world.slots[moved.index].row = row;
}

/* Drops everything the components of one row own. */
static void table_drop_row(MnTable *table, uint32_t row) {
    const MnComponentInfo *info = mn_world.program->components;
    for (uint32_t i = 0; i < table->drop_count; i++) {
        uint32_t c = table->drops[i];
        info[c].drop((char *)table->columns[c] + (size_t)row * info[c].size);
    }
}

/* ---- Structural changes, applied at once -------------------------------- */

/* Drops the values of a bundle that will not be stored. */
static void bundle_drop(const MnBundle *bundle, void *const *values) {
    const MnComponentInfo *info = mn_world.program->components;
    if (!values) return;
    for (uint32_t i = 0; i < bundle->count; i++) {
        const MnComponentInfo *c = &info[bundle->components[i]];
        if (c->size != 0 && c->drop && values[i]) c->drop(values[i]);
    }
}

/* Puts an entity that is in no table into the bundle's table. */
static void spawn_now(MnEntity e, MnBundle *bundle, void *const *values) {
    MnWorld *w = &mn_world;
    const MnComponentInfo *info = w->program->components;
    uint32_t id = bundle_table(w->tables[0], bundle);
    MnTable *table = w->tables[id];
    uint32_t row = table_append(table, e);
    MnSlot *slot = &w->slots[e.index];
    slot->table = id;
    slot->row = row;
    for (uint32_t i = 0; i < bundle->count; i++) {
        uint32_t c = bundle->components[i];
        size_t size = info[c].size;
        if (size == 0) continue;
        char *to = (char *)table->columns[c] + row * size;
        const void *from = values ? values[i] : NULL;
        if (from) {
            memcpy(to, from, size);
        } else {
            memset(to, 0, size);
        }
    }
    w->live_entities++;
}

static void add_now(MnEntity e, MnBundle *bundle, void *const *values) {
    MnWorld *w = &mn_world;
    const MnComponentInfo *info = w->program->components;
    if (!mn_alive(e) || w->slots[e.index].table == MN_NO_TABLE) {
        bundle_drop(bundle, values);
        return;
    }
    MnSlot *slot = &w->slots[e.index];
    MnTable *from = w->tables[slot->table];
    uint32_t from_row = slot->row;
    uint32_t id = bundle_table(from, bundle);
    MnTable *to = w->tables[id];
    uint32_t to_row = from_row;
    if (to != from) {
        table_moving(from, from_row, NULL, 0);
        to_row = table_append(to, e);
        for (uint32_t i = 0; i < from->data_count; i++) {
            uint32_t c = from->data[i];
            size_t size = from->data_sizes[i];
            memcpy((char *)to->columns[c] + to_row * size,
                   (char *)from->columns[c] + from_row * size, size);
        }
    }
    for (uint32_t i = 0; i < bundle->count; i++) {
        uint32_t c = bundle->components[i];
        size_t size = info[c].size;
        if (size == 0) continue;
        if (to == from) table_replacing(to, to_row, c);
        char *value = (char *)to->columns[c] + to_row * size;
        if (from->columns[c] && info[c].drop) info[c].drop(value);
        const void *given = values ? values[i] : NULL;
        if (given) {
            memcpy(value, given, size);
        } else {
            memset(value, 0, size);
        }
    }
    if (to != from) {
        table_remove(from, from_row);
        slot->table = id;
        slot->row = to_row;
    }
}

static void remove_now(MnEntity e, const uint32_t *components,
                       uint32_t count) {
    MnWorld *w = &mn_world;
    const MnComponentInfo *info = w->program->components;
    if (!mn_alive(e) || w->slots[e.index].table == MN_NO_TABLE) return;
    MnSlot *slot = &w->slots[e.index];
    MnTable *from = w->tables[slot->table];
    uint32_t from_row = slot->row;
    uint32_t id = table_without(from, components, count);
    if (id == from->id) return;
    MnTable *to = w->tables[id];
    table_moving(from, from_row, components, count);
    uint32_t to_row = table_append(to, e);
    for (uint32_t i = 0; i < to->data_count; i++) {
        uint32_t c = to->data[i];
        size_t size = to->data_sizes[i];
        memcpy((char *)to->columns[c] + to_row * size,
               (char *)from->columns[c] + from_row * size, size);
    }
    for (uint32_t i = 0; i < from->drop_count; i++) {
        uint32_t c = from->drops[i];
        if (to->columns[c]) continue;
        info[c].drop((char *)from->columns[c] +
                     (size_t)from_row * info[c].size);
    }
    table_remove(from, from_row);
    slot->table = id;
    slot->row = to_row;
}

static void despawn_now(MnEntity e) {
    MnWorld *w = &mn_world;
    if (!mn_alive(e)) return;
    MnSlot *slot = &w->slots[e.index];
    if (slot->table != MN_NO_TABLE) {
        MnTable *table = w->tables[slot->table];
        if (table->drop_count != 0) table_drop_row(table, slot->row);
        table_remove(table, slot->row);
        w->live_entities--;
    }
    slot_free(e.index);
}

/* ---- Structural changes, queued ----------------------------------------- */

enum { MN_CMD_SPAWN, MN_CMD_ADD, MN_CMD_REMOVE, MN_CMD_DESPAWN };

/* The start of every queued record. A spawn or add record continues with
 * the bundle's address and then each of its values with storage, in bundle
 * order; a remove record continues with `count` component ids. Every part
 * starts at an 8-byte boundary. */
typedef struct MnCommand {
    uint32_t kind;
    uint32_t count;
    MnEntity entity;
} MnCommand;

/* A queue larger than this is released once it has been applied. */
#define MN_COMMANDS_KEPT ((size_t)1 << 20)

static size_t round8(size_t size) { return (size + 7) & ~(size_t)7; }

/* Makes the world's queue `need` bytes longer and returns where they
 * start. */
static unsigned char *commands_extend(size_t need) {
    MnWorld *w = &mn_world;
    if (w->command_capacity - w->command_len < need) {
        size_t capacity = w->command_capacity ? w->command_capacity : 1024;
        while (capacity - w->command_len < need) {
            if (capacity > SIZE_MAX / 2) mn_out_of_memory();
            capacity *= 2;
        }
        w->commands = mn_realloc(w->commands, capacity);
        w->command_capacity = capacity;
    }
    unsigned char *at = w->commands + w->command_len;
    w->command_len += need;
    return at;
}

/* Appends a record with `payload` bytes after its start and returns the
 * address of the payload. In a piece the record is kept with the piece. */
static unsigned char *command_push(uint32_t kind, uint32_t count, MnEntity e,
                                   size_t payload) {
    size_t need = sizeof(MnCommand) + payload;
    unsigned char *at;
#ifdef MN_THREADS
    if (mn_loop_thread) {
        at = piece_bytes(mn_loop_thread, 0, need);
    } else
#endif
    {
        at = commands_extend(need);
        mn_world.deferred_count++;
    }
    MnCommand command = {kind, count, e};
    memcpy(at, &command, sizeof command);
    return at + sizeof command;
}

/* Queues a spawn or an add, copying the values in. */
static void command_push_bundle(uint32_t kind, MnEntity e, MnBundle *bundle,
                                void *const *values) {
    const MnComponentInfo *info = mn_world.program->components;
    size_t payload = round8(sizeof bundle);
    for (uint32_t i = 0; i < bundle->count; i++) {
        payload += round8(info[bundle->components[i]].size);
    }
    unsigned char *at = command_push(kind, bundle->count, e, payload);
    memcpy(at, &bundle, sizeof bundle);
    at += round8(sizeof bundle);
    for (uint32_t i = 0; i < bundle->count; i++) {
        size_t size = info[bundle->components[i]].size;
        if (size == 0) continue;
        const void *from = values ? values[i] : NULL;
        if (from) {
            memcpy(at, from, size);
        } else {
            memset(at, 0, size);
        }
        at += round8(size);
    }
}

#ifdef MN_GPU
/* Brings back, in one round trip, the rows that the queued `add`s and
 * `remove`s will move to another table and that only a GPU copy has. A row
 * brought back stays known as it moves, so applying the queue needs no
 * round trip of its own. */
static void commands_want_rows(void) {
    MnWorld *w = &mn_world;
    const MnComponentInfo *info = w->program->components;
    unsigned char *at = w->commands;
    unsigned char *end = at + w->command_len;
    while (at < end) {
        MnCommand command;
        memcpy(&command, at, sizeof command);
        at += sizeof command;
        MnEntity e = command.entity;
        MnTable *table = NULL;
        uint32_t row = 0;
        if (mn_alive(e) && w->slots[e.index].table != MN_NO_TABLE) {
            table = w->tables[w->slots[e.index].table];
            row = w->slots[e.index].row;
        }
        bool moves = false;
        switch (command.kind) {
        case MN_CMD_SPAWN:
        case MN_CMD_ADD: {
            MnBundle *bundle;
            memcpy(&bundle, at, sizeof bundle);
            at += round8(sizeof bundle);
            for (uint32_t i = 0; i < bundle->count; i++) {
                uint32_t c = bundle->components[i];
                at += round8(info[c].size);
                if (table && !table->columns[c]) moves = true;
            }
            if (command.kind == MN_CMD_ADD && moves) {
                mn_gpu_want_row(table, row, NULL, 0);
            }
            break;
        }
        case MN_CMD_REMOVE: {
            const uint32_t *components = (const uint32_t *)(void *)at;
            for (uint32_t i = 0; i < command.count; i++) {
                if (table && table->columns[components[i]]) moves = true;
            }
            if (moves) mn_gpu_want_row(table, row, components, command.count);
            at += round8((size_t)command.count * sizeof(uint32_t));
            break;
        }
        default:
            break;
        }
    }
    mn_gpu_fetch();
}
#endif

/* Goes through the queue in order and empties it. With `apply`, each
 * change is made; without, the queued values are only dropped. */
static void commands_run(bool apply) {
    MnWorld *w = &mn_world;
    const MnComponentInfo *info = w->program->components;
#ifdef MN_GPU
    if (apply && mn_gpu_only) commands_want_rows();
#endif
    unsigned char *at = w->commands;
    unsigned char *end = at + w->command_len;
    while (at < end) {
        MnCommand command;
        memcpy(&command, at, sizeof command);
        at += sizeof command;
        MnEntity e = command.entity;
        switch (command.kind) {
        case MN_CMD_SPAWN:
        case MN_CMD_ADD: {
            MnBundle *bundle;
            memcpy(&bundle, at, sizeof bundle);
            at += round8(sizeof bundle);
            void **values = w->scratch_values;
            for (uint32_t i = 0; i < bundle->count; i++) {
                size_t size = info[bundle->components[i]].size;
                values[i] = size ? at : NULL;
                at += round8(size);
            }
            if (!apply) {
                bundle_drop(bundle, values);
            } else if (command.kind == MN_CMD_ADD) {
                add_now(e, bundle, values);
            } else if (mn_alive(e) &&
                       w->slots[e.index].table == MN_NO_TABLE) {
                spawn_now(e, bundle, values);
            } else {
                bundle_drop(bundle, values);
            }
            break;
        }
        case MN_CMD_REMOVE:
            if (apply) {
                remove_now(e, (const uint32_t *)(void *)at, command.count);
            }
            at += round8((size_t)command.count * sizeof(uint32_t));
            break;
        default:
            if (apply) despawn_now(e);
            break;
        }
    }
    w->command_len = 0;
    w->deferred_count = 0;
    if (w->command_capacity > MN_COMMANDS_KEPT) {
        free(w->commands);
        w->commands = NULL;
        w->command_capacity = 0;
    }
}

void mn_flush(void) { commands_run(true); }

MnEntity mn_spawn(MnBundle *bundle, void *const *values) {
    MnEntity e = slot_take();
    if (mn_world.defer_depth != 0) {
        command_push_bundle(MN_CMD_SPAWN, e, bundle, values);
    } else {
        spawn_now(e, bundle, values);
    }
    return e;
}

void mn_add(MnEntity e, MnBundle *bundle, void *const *values) {
    if (mn_world.defer_depth == 0) {
        add_now(e, bundle, values);
    } else if (mn_alive(e)) {
        command_push_bundle(MN_CMD_ADD, e, bundle, values);
    } else {
        bundle_drop(bundle, values);
    }
}

void mn_remove(MnEntity e, const uint32_t *components, uint32_t count) {
    if (mn_world.defer_depth == 0) {
        remove_now(e, components, count);
    } else if (mn_alive(e) && count != 0) {
        size_t bytes = (size_t)count * sizeof(uint32_t);
        unsigned char *at =
            command_push(MN_CMD_REMOVE, count, e, round8(bytes));
        memcpy(at, components, bytes);
    }
}

void mn_despawn(MnEntity e) {
    if (mn_world.defer_depth == 0) {
        despawn_now(e);
    } else if (mn_alive(e)) {
        command_push(MN_CMD_DESPAWN, 0, e, 0);
    }
}

/* ---- Queries ------------------------------------------------------------- */

void mn_query_refresh(MnQuery *query) {
    MnWorld *w = &mn_world;
    for (uint32_t t = query->tables_seen; t < w->table_count; t++) {
        MnTable *table = w->tables[t];
        bool matches = true;
        for (uint32_t i = 0; matches && i < query->with_count; i++) {
            matches = table->columns[query->with[i]] != NULL;
        }
        for (uint32_t i = 0; matches && i < query->without_count; i++) {
            matches = table->columns[query->without[i]] == NULL;
        }
        if (!matches) continue;
        if (query->table_count == query->table_capacity) {
            if (query->table_capacity > UINT32_MAX / 2) mn_out_of_memory();
            uint32_t capacity =
                query->table_capacity ? query->table_capacity * 2 : 4;
            query->tables = mn_realloc(query->tables,
                                       (size_t)capacity * sizeof(MnTable *));
            query->table_capacity = capacity;
        }
        query->tables[query->table_count++] = table;
    }
    query->tables_seen = w->table_count;
}

int64_t mn_query_count(MnQuery *query) {
    mn_query_update(query);
    int64_t count = 0;
    for (uint32_t i = 0; i < query->table_count; i++) {
        count += query->tables[i]->count;
    }
    return count;
}

int64_t mn_entity_count(void) { return mn_world.live_entities; }

/* ---- Events -------------------------------------------------------------- */

/* The events of one type that have not been discarded: those numbered
 * [base, end), stored from the start of `data`. */
typedef struct MnEventBuf {
    char *data;
    uint64_t base;
    uint64_t end;
    /* `end` as it was when the previous tick finished. */
    uint64_t mark;
    uint64_t capacity; /* in events */
} MnEventBuf;

/* Makes room for `more` events of one type after the newest and returns
 * where the first goes. The caller fills them in. */
static void *events_extend(uint32_t event, uint64_t more) {
    MnEventBuf *buf = &mn_world.events[event];
    size_t size = mn_world.program->events[event].size;
    uint64_t count = buf->end - buf->base;
    if (buf->capacity - count < more) {
        uint64_t capacity = buf->capacity ? buf->capacity : 16;
        while (capacity - count < more) {
            if (capacity > SIZE_MAX / 2) mn_out_of_memory();
            capacity *= 2;
        }
        buf->data = mn_realloc(buf->data, mn_mul((size_t)capacity, size));
        buf->capacity = capacity;
    }
    buf->end += more;
    return buf->data + count * size;
}

void *mn_emit(uint32_t event) {
    size_t size = mn_world.program->events[event].size;
    void *slot;
#ifdef MN_THREADS
    /* In a piece the event is kept with the piece. */
    if (mn_loop_thread) {
        slot = piece_bytes(mn_loop_thread, 1 + event, size);
    } else
#endif
    {
        slot = events_extend(event, 1);
    }
    memset(slot, 0, size);
    return slot;
}

uint64_t mn_event_end(uint32_t event) { return mn_world.events[event].end; }

void *mn_event_at(uint32_t event, uint64_t seq) {
    const MnEventBuf *buf = &mn_world.events[event];
    if (seq < buf->base || seq >= buf->end) return NULL;
    return buf->data + (seq - buf->base) * mn_world.program->events[event].size;
}

/* Drops and removes the events of one type numbered below `until`. */
static void events_discard(uint32_t event, uint64_t until) {
    MnEventBuf *buf = &mn_world.events[event];
    const MnEventInfo *info = &mn_world.program->events[event];
    size_t size = info->size;
    size_t gone = (size_t)(until - buf->base);
    if (gone == 0) return;
    if (info->drop) {
        for (size_t i = 0; i < gone; i++) info->drop(buf->data + i * size);
    }
    size_t kept = (size_t)(buf->end - until);
    if (kept != 0) memmove(buf->data, buf->data + gone * size, kept * size);
    buf->base = until;
}

/* The words of MIDNIGHT_GPU. */
static struct {
    bool off;
    bool software;
    bool info;
    bool stats;
    bool rows;
    bool strict_cpu;
} gpu_options;

/* ---- The window ------------------------------------------------------------ */

#ifdef MN_WINDOW
const MnWindowProgram *mn_window_program;

/* The window as the runtime knows it. `open` is true from Window.open to
 * the tick that reports the close; the sizes are what the desktop gives
 * now, and reach the resource when a tick begins. */
static struct {
    bool headless;
    bool info;

    bool open;
    bool closing;
    int64_t width;
    int64_t height;
    int64_t pixel_width;
    int64_t pixel_height;
    /* A headless window's scale: `scale_num / scale_den` pixels a point. */
    int64_t scale_num;
    int64_t scale_den;

    /* MIDNIGHT_DRIVE, or NULL. */
    const char *drive_name;
    FILE *drive;
    /* The commands have run out. */
    bool drive_done;
    int64_t drive_line;
    char *line;
    size_t line_capacity;
    /* The `tick` command being carried out: its text, and how many of its
     * ticks are still to begin and have begun. */
    char *tick_text;
    int64_t ticks_left;
    int64_t ticks_run;

    /* The last frame: its size in pixels and in points, its background,
     * and the value of Time.tick when it was made. Its draws are
     * drawing.frame. */
    uint32_t frame_width;
    uint32_t frame_height;
    int64_t frame_point_width;
    int64_t frame_point_height;
    MnColor frame_background;
    int64_t frame_tick;

    /* The window is on the desktop, through midnight_window.c. */
    bool shown;
    /* When the last tick of a shown window was due, in nanoseconds of the
     * monotonic clock; `paced` is false until the first tick after
     * Window.open. */
    bool paced;
    int64_t due;
} win;

/* Prints a command's answer. */
static void drive_answer(const char *format, ...)
    __attribute__((format(printf, 1, 2)));
static void drive_answer(const char *format, ...) {
    va_list args;
    fflush(stdout);
    fputs("midnight: drive: ", stderr);
    va_start(args, format);
    vfprintf(stderr, format, args);
    va_end(args);
    fputc('\n', stderr);
}

/* Reports a command that can't be carried out and ends the program. */
static _Noreturn void drive_fail(const char *format, ...)
    __attribute__((format(printf, 1, 2)));
static _Noreturn void drive_fail(const char *format, ...) {
    va_list args;
    fflush(stdout);
    fprintf(stderr, "midnight: drive: %s:%lld: ", win.drive_name,
            (long long)win.drive_line);
    va_start(args, format);
    vfprintf(stderr, format, args);
    va_end(args);
    fputc('\n', stderr);
    mn_leave(1);
}

/* ---- Drawing ---- */

/* An array of draw records. */
typedef struct MnDraws {
    MnDraw *data;
    size_t len;
    size_t capacity;
} MnDraws;

static struct {
    /* The draws since the last frame, in the order they were made. */
    MnDraws list;
    /* The draws of the last frame. */
    MnDraws frame;
} drawing;

/* What a draw is given while no window is open. */
#ifdef MN_THREADS
static _Thread_local MnDraw draw_dropped;
#else
static MnDraw draw_dropped;
#endif

/* Adds `more` records to the end of `draws` and returns the first. */
static MnDraw *draws_extend(MnDraws *draws, size_t more) {
    if (draws->capacity - draws->len < more) {
        size_t capacity = draws->capacity ? draws->capacity : 64;
        while (capacity - draws->len < more) {
            if (capacity > SIZE_MAX / 2 / sizeof(MnDraw)) mn_out_of_memory();
            capacity *= 2;
        }
        draws->data = mn_realloc(draws->data, capacity * sizeof(MnDraw));
        draws->capacity = capacity;
    }
    MnDraw *first = draws->data + draws->len;
    draws->len += more;
    return first;
}

MnDraw *mn_draw(void) {
    MnDraw *record;
    if (!win.open) {
        record = &draw_dropped;
    }
#ifdef MN_THREADS
    /* In a piece the draw is kept with the piece. */
    else if (mn_loop_thread) {
        record = (MnDraw *)(void *)piece_bytes(
            mn_loop_thread, 1 + mn_world.program->event_count, sizeof(MnDraw));
    }
#endif
    else {
        record = draws_extend(&drawing.list, 1);
    }
    memset(record, 0, sizeof *record);
    return record;
}

/* ---- The keyboard and the mouse ---- */

/* Something that happened to the keyboard, the mouse or the focus. `which`
 * is the number of a `Key` or `MouseButton` variant; `x` and `y` are a
 * position in points, or the wheel's notches. */
typedef enum {
    HAPPENED_KEY_DOWN,
    HAPPENED_KEY_UP,
    HAPPENED_BUTTON_DOWN,
    HAPPENED_BUTTON_UP,
    HAPPENED_MOVE,
    HAPPENED_LEAVE,
    HAPPENED_WHEEL,
    HAPPENED_BLUR,
    HAPPENED_FOCUS,
    HAPPENED_CLOSE,
} HappenedKind;

typedef struct {
    HappenedKind kind;
    uint32_t which;
    double x;
    double y;
} Happening;

/* The keys and buttons held, where the pointer is, whether the last thing
 * it did was a leave, and the focus. */
typedef struct {
    bool keys[MN_KEY_COUNT];
    bool buttons[MN_MOUSE_BUTTON_COUNT];
    double x;
    double y;
    bool left;
    bool focused;
} InputHeld;

static struct {
    /* What happened since the last tick began, in order. */
    Happening *list;
    size_t count;
    size_t capacity;

    /* The state when the tick began. */
    InputHeld now;
    bool key_pressed[MN_KEY_COUNT];
    bool key_released[MN_KEY_COUNT];
    bool button_pressed[MN_MOUSE_BUTTON_COUNT];
    bool button_released[MN_MOUSE_BUTTON_COUNT];
    double wheel_x;
    double wheel_y;

    /* The state the commands read so far leave. */
    InputHeld commands;

    /* The pointer has left the shown window, as the desktop says. */
    bool desktop_left;
} input;

static const char *const key_names[MN_KEY_COUNT] = {
#define MN_KEY_NAME(name, usage) #name,
    MN_KEYS(MN_KEY_NAME)
#undef MN_KEY_NAME
};

static const char *const button_names[MN_MOUSE_BUTTON_COUNT] = {
#define MN_BUTTON_NAME(name, number) #name,
    MN_MOUSE_BUTTONS(MN_BUTTON_NAME)
#undef MN_BUTTON_NAME
};

/* The number of the name among `count` names, or -1. */
static int input_named(const char *const *names, int count, const char *name) {
    for (int i = 0; i < count; i++) {
        if (strcmp(names[i], name) == 0) return i;
    }
    return -1;
}

static void input_happened(HappenedKind kind, uint32_t which, double x, double y) {
    if (input.count == input.capacity) {
        input.capacity = input.capacity ? input.capacity * 2 : 64;
        input.list = mn_realloc(input.list, input.capacity * sizeof *input.list);
    }
    input.list[input.count++] = (Happening){kind, which, x, y};
}

/* Whether a position is within the window's size now. */
static bool input_within(double x, double y) {
    return x >= 0.0 && y >= 0.0 && x < (double)win.width && y < (double)win.height;
}

static bool input_any_button(const InputHeld *held) {
    for (int i = 0; i < MN_MOUSE_BUTTON_COUNT; i++) {
        if (held->buttons[i]) return true;
    }
    return false;
}

/* The state of a window that has just opened: nothing held, the pointer
 * outside at 0, 0, and no focus. */
static void input_opened(InputHeld *held) {
    memset(held, 0, sizeof *held);
    held->left = true;
}

/* Releases what is held, keys in the order of `Key`, then buttons in the
 * order of `MouseButton`, emitting their events. */
static void input_release_all(void) {
    const MnWindowProgram *program = mn_window_program;
    for (uint32_t key = 0; key < MN_KEY_COUNT; key++) {
        if (!input.now.keys[key]) continue;
        input.now.keys[key] = false;
        input.key_released[key] = true;
        MnKeyEvent *event = mn_emit(program->key_released);
        event->key = (uint8_t)key;
    }
    for (uint32_t button = 0; button < MN_MOUSE_BUTTON_COUNT; button++) {
        if (!input.now.buttons[button]) continue;
        input.now.buttons[button] = false;
        input.button_released[button] = true;
        MnButtonEvent *event = mn_emit(program->mouse_released);
        event->button = (uint8_t)button;
        event->x = input.now.x;
        event->y = input.now.y;
    }
}

/* What a close does to the state: everything held is released, and there
 * is no focus and no pointer over the window. */
static void input_close(void) {
    input_release_all();
    input.now.focused = false;
    input.now.left = true;
}

/* Applies what happened since the last tick began, in order, emitting
 * the events. Nothing after a close is applied. */
static void input_apply(void) {
    const MnWindowProgram *program = mn_window_program;
    for (size_t i = 0; i < input.count; i++) {
        const Happening *h = &input.list[i];
        switch (h->kind) {
        case HAPPENED_KEY_DOWN:
        case HAPPENED_KEY_UP: {
            bool down = h->kind == HAPPENED_KEY_DOWN;
            if (input.now.keys[h->which] == down) break;
            input.now.keys[h->which] = down;
            (down ? input.key_pressed : input.key_released)[h->which] = true;
            MnKeyEvent *event =
                mn_emit(down ? program->key_pressed : program->key_released);
            event->key = (uint8_t)h->which;
            break;
        }
        case HAPPENED_BUTTON_DOWN:
        case HAPPENED_BUTTON_UP: {
            bool down = h->kind == HAPPENED_BUTTON_DOWN;
            input.now.x = h->x;
            input.now.y = h->y;
            if (input.now.buttons[h->which] == down) break;
            input.now.buttons[h->which] = down;
            (down ? input.button_pressed : input.button_released)[h->which] = true;
            MnButtonEvent *event =
                mn_emit(down ? program->mouse_pressed : program->mouse_released);
            event->button = (uint8_t)h->which;
            event->x = h->x;
            event->y = h->y;
            break;
        }
        case HAPPENED_MOVE:
            input.now.x = h->x;
            input.now.y = h->y;
            input.now.left = false;
            break;
        case HAPPENED_LEAVE:
            input.now.left = true;
            break;
        case HAPPENED_WHEEL:
            input.wheel_x += h->x;
            input.wheel_y += h->y;
            break;
        case HAPPENED_BLUR:
            if (!input.now.focused) break;
            input_release_all();
            input.now.focused = false;
            mn_emit(program->unfocused);
            break;
        case HAPPENED_FOCUS:
            if (input.now.focused) break;
            input.now.focused = true;
            mn_emit(program->focused);
            break;
        case HAPPENED_CLOSE:
            input_close();
            input.count = 0;
            return;
        }
    }
    input.count = 0;
}

/* Clears what was pressed and released and the wheel, as every tick
 * begins. */
static void input_clear(void) {
    memset(input.key_pressed, 0, sizeof input.key_pressed);
    memset(input.key_released, 0, sizeof input.key_released);
    memset(input.button_pressed, 0, sizeof input.button_pressed);
    memset(input.button_released, 0, sizeof input.button_released);
    input.wheel_x = 0.0;
    input.wheel_y = 0.0;
}

/* Assigns the fields of `Mouse` and `Window.focused` from the state. */
static void input_assign(void) {
    MnMouse *mouse = mn_window_program->mouse;
    mouse->x = input.now.x;
    mouse->y = input.now.y;
    mouse->wheel_x = input.wheel_x;
    mouse->wheel_y = input.wheel_y;
    mouse->inside = !input.now.left && input_within(input.now.x, input.now.y);
    mn_window_program->window->focused = input.now.focused;
}

bool mn_key_down(uint32_t key) { return key < MN_KEY_COUNT && input.now.keys[key]; }

bool mn_key_pressed(uint32_t key) { return key < MN_KEY_COUNT && input.key_pressed[key]; }

bool mn_key_released(uint32_t key) {
    return key < MN_KEY_COUNT && input.key_released[key];
}

bool mn_button_down(uint32_t button) {
    return button < MN_MOUSE_BUTTON_COUNT && input.now.buttons[button];
}

bool mn_button_pressed(uint32_t button) {
    return button < MN_MOUSE_BUTTON_COUNT && input.button_pressed[button];
}

bool mn_button_released(uint32_t button) {
    return button < MN_MOUSE_BUTTON_COUNT && input.button_released[button];
}

#ifdef MN_FRAMES
/* What the desktop says. A driven window takes its input from its
 * commands, and these drop what they are given. */

void mn_window_key(uint32_t key, bool down) {
    if (win.drive_name || key >= MN_KEY_COUNT) return;
    input_happened(down ? HAPPENED_KEY_DOWN : HAPPENED_KEY_UP, key, 0.0, 0.0);
}

void mn_window_button(uint32_t button, bool down, double x, double y) {
    if (win.drive_name || button >= MN_MOUSE_BUTTON_COUNT) return;
    input_happened(down ? HAPPENED_BUTTON_DOWN : HAPPENED_BUTTON_UP, button, x, y);
}

void mn_window_move(double x, double y) {
    if (win.drive_name) return;
    if (input.desktop_left && win.info) {
        fflush(stdout);
        fputs("midnight: window: pointer inside\n", stderr);
    }
    input.desktop_left = false;
    input_happened(HAPPENED_MOVE, 0, x, y);
}

void mn_window_leave(void) {
    if (win.drive_name) return;
    input.desktop_left = true;
    input_happened(HAPPENED_LEAVE, 0, 0.0, 0.0);
}

void mn_window_wheel(double x, double y) {
    if (win.drive_name) return;
    input_happened(HAPPENED_WHEEL, 0, x, y);
}

void mn_window_focus(bool gained) {
    if (win.drive_name) return;
    if (gained && win.info) {
        fflush(stdout);
        fputs("midnight: window: focused\n", stderr);
    }
    input_happened(gained ? HAPPENED_FOCUS : HAPPENED_BLUR, 0, 0.0, 0.0);
}
#endif

/* ---- Commands of the keyboard and the mouse ---- */

/* A number of a command: an optional `-`, one or more digits, and
 * optionally a point and one to eight digits, from -most to most. Sets
 * `out` to the Float nearest to it and returns true, or returns false. */
static bool drive_number(const char *word, int64_t most, double *out) {
    static const int64_t tens[9] = {1,      10,      100,      1000,     10000,
                                    100000, 1000000, 10000000, 100000000};
    bool negative = *word == '-';
    if (negative) word++;
    int64_t digits = 0;
    int places = -1;
    int count = 0;
    for (const char *c = word; *c != '\0'; c++) {
        if (*c == '.' && places < 0 && count > 0) {
            places = 0;
            continue;
        }
        if (*c < '0' || *c > '9') return false;
        if (places >= 0 && ++places > 8) return false;
        count++;
        digits = digits * 10 + (*c - '0');
        if (digits > most * tens[8]) return false;
    }
    if (count == 0 || places == 0) return false;
    if (places < 0) places = 0;
    if (digits > most * tens[places]) return false;
    double value = (double)digits / (double)tens[places];
    *out = negative && digits != 0 ? -value : value;
    return true;
}

/* Carries out `key …`, `mouse …`, `scroll …`, `blur` and `focus`, given
 * as words; returns false for any other command. */
static bool drive_input(const char *text, char **words, int count) {
    InputHeld *held = &input.commands;
    const char *name = words[0];
    if (strcmp(name, "key") == 0) {
        int mode = 0; /* 1 down, 2 up, 3 both */
        const char *key_name = NULL;
        if (count == 3 && strcmp(words[1], "down") == 0) {
            mode = 1;
        } else if (count == 3 && strcmp(words[1], "up") == 0) {
            mode = 2;
        } else if (count == 2) {
            mode = 3;
        }
        if (mode == 0) {
            drive_fail("`key` takes `down` or `up` and a key, or a key");
        }
        key_name = words[count - 1];
        int key = input_named(key_names, MN_KEY_COUNT, key_name);
        if (key < 0) drive_fail("`%s` is not a key", key_name);
        if (!held->focused) {
            drive_fail("`%s` while the window does not have the focus", text);
        }
        if (mode != 2 && held->keys[key]) drive_fail("`%s` is already down", key_name);
        if (mode == 2 && !held->keys[key]) drive_fail("`%s` is already up", key_name);
        if (mode != 2) input_happened(HAPPENED_KEY_DOWN, (uint32_t)key, 0.0, 0.0);
        if (mode != 1) input_happened(HAPPENED_KEY_UP, (uint32_t)key, 0.0, 0.0);
        held->keys[key] = mode == 1;
    } else if (strcmp(name, "mouse") == 0) {
        bool over = !held->left && input_within(held->x, held->y);
        if (count >= 2 && strcmp(words[1], "move") == 0) {
            double x;
            double y;
            if (count != 4 || !drive_number(words[2], MN_WINDOW_LARGEST, &x) ||
                !drive_number(words[3], MN_WINDOW_LARGEST, &y)) {
                drive_fail("`mouse move` takes a position in points, each a number "
                           "from -%d to %d",
                           MN_WINDOW_LARGEST, MN_WINDOW_LARGEST);
            }
            if (!input_within(x, y) && !input_any_button(held)) {
                drive_fail("`%s` is outside the window of %lldx%lld points while no "
                           "button is held; a pointer that leaves is `mouse leave`",
                           text, (long long)win.width, (long long)win.height);
            }
            held->x = x;
            held->y = y;
            held->left = false;
            input_happened(HAPPENED_MOVE, 0, x, y);
        } else if (count == 2 && strcmp(words[1], "leave") == 0) {
            if (!over) drive_fail("`mouse leave` while the pointer is not over the window");
            held->left = true;
            input_happened(HAPPENED_LEAVE, 0, 0.0, 0.0);
        } else {
            int mode = 0; /* 1 down, 2 up, 3 both */
            if (count == 3 && strcmp(words[1], "down") == 0) {
                mode = 1;
            } else if (count == 3 && strcmp(words[1], "up") == 0) {
                mode = 2;
            } else if (count == 2) {
                mode = 3;
            }
            if (mode == 0) {
                drive_fail("`mouse` takes `move` and a position, `down` or `up` and a "
                           "button, `leave`, or a button");
            }
            const char *button_name = words[count - 1];
            int button = input_named(button_names, MN_MOUSE_BUTTON_COUNT, button_name);
            if (button < 0) drive_fail("`%s` is not a mouse button", button_name);
            if (mode != 2 && held->buttons[button]) {
                drive_fail("`%s` is already down", button_name);
            }
            if (mode == 2 && !held->buttons[button]) {
                drive_fail("`%s` is already up", button_name);
            }
            if (mode != 2 && !over && !input_any_button(held)) {
                drive_fail("`%s` while the pointer is not over the window and no "
                           "button is held",
                           text);
            }
            if (mode != 2) {
                input_happened(HAPPENED_BUTTON_DOWN, (uint32_t)button, held->x, held->y);
            }
            if (mode != 1) {
                input_happened(HAPPENED_BUTTON_UP, (uint32_t)button, held->x, held->y);
            }
            held->buttons[button] = mode == 1;
        }
    } else if (strcmp(name, "scroll") == 0) {
        double x;
        double y;
        if (count != 3 || !drive_number(words[1], 1000, &x) ||
            !drive_number(words[2], 1000, &y)) {
            drive_fail("`scroll` takes two numbers of notches, each from -1000 to 1000");
        }
        input_happened(HAPPENED_WHEEL, 0, x, y);
    } else if (strcmp(name, "blur") == 0 || strcmp(name, "focus") == 0) {
        bool focus = name[0] == 'f';
        if (count != 1) drive_fail("`%s` takes nothing", name);
        if (focus && held->focused) {
            drive_fail("`focus` while the window has the focus");
        }
        if (!focus && !held->focused) {
            drive_fail("`blur` while the window does not have the focus");
        }
        if (!focus) {
            memset(held->keys, 0, sizeof held->keys);
            memset(held->buttons, 0, sizeof held->buttons);
        }
        held->focused = focus;
        input_happened(focus ? HAPPENED_FOCUS : HAPPENED_BLUR, 0, 0.0, 0.0);
    } else {
        return false;
    }
    drive_answer("%s", text);
    return true;
}

static void gpu_read_options(void);

#ifdef MN_FRAMES
static const char *gpu_device_start(struct WGPUSurfaceImpl *surface);

const char *mn_window_device(struct WGPUSurfaceImpl *surface) {
    return gpu_device_start(surface);
}
#endif

/* Reads MIDNIGHT_WINDOW, MIDNIGHT_DRIVE and MIDNIGHT_GPU. */
static void window_start(void) {
    gpu_read_options();
    const char *list = getenv("MIDNIGHT_WINDOW");
    while (list && *list != '\0') {
        size_t len = strcspn(list, ",");
        if (len == 8 && memcmp(list, "headless", 8) == 0) {
            win.headless = true;
        } else if (len == 4 && memcmp(list, "info", 4) == 0) {
            win.info = true;
        } else if (len != 0) {
            fprintf(stderr, "midnight: MIDNIGHT_WINDOW: unknown word `%.*s`\n",
                    (int)len, list);
        }
        list += len;
        if (*list == ',') list++;
    }
    const char *drive = getenv("MIDNIGHT_DRIVE");
    if (drive && *drive != '\0') win.drive_name = drive;
}

/* The pixels of `points` at the headless window's scale, halves rounded
 * up. */
static int64_t scaled(int64_t points) {
    return (points * win.scale_num * 2 + win.scale_den) / (win.scale_den * 2);
}

/* A whole number from 1 to `most`, the whole of `word`, or -1. */
static int64_t drive_count(const char *word, int64_t most) {
    int64_t value = 0;
    if (*word == '\0') return -1;
    for (const char *c = word; *c != '\0'; c++) {
        if (*c < '0' || *c > '9') return -1;
        value = value * 10 + (*c - '0');
        if (value > most) return -1;
    }
    return value < 1 ? -1 : value;
}

/* The command's words, split at spaces and tabs, in place. Returns how
 * many there are, at most `most`; one more means too many. */
static int drive_words(char *text, char **words, int most) {
    int count = 0;
    char *at = text;
    while (*at != '\0') {
        while (*at == ' ' || *at == '\t') *at++ = '\0';
        if (*at == '\0') break;
        if (count == most) return most + 1;
        words[count++] = at;
        while (*at != '\0' && *at != ' ' && *at != '\t') at++;
    }
    return count;
}

static void window_capture(const char *text, const char *file);
static int format_number(double value, bool single, char *out);

/* Answers `draws`: the last frame as text, a line for the frame, one for
 * its background and one for each draw that is not of nothing. */
static void window_draws(void) {
    static const char *const shapes[] = {"", "rect", "circle", "line", "triangle"};
    static const int numbers[] = {0, 4, 3, 5, 6};
    if (!win.open) drive_fail("can't list the draws: no window is open");
    const MnDraws *frame = &drawing.frame;
    size_t listed = 0;
    for (size_t i = 0; i < frame->len; i++) {
        if (frame->data[i].kind != MN_DRAW_NOTHING) listed++;
    }
    drive_answer("draws: %lld tick%s, %lldx%lld, %zu draw%s",
                 (long long)win.frame_tick, win.frame_tick == 1 ? "" : "s",
                 (long long)win.frame_point_width,
                 (long long)win.frame_point_height, listed,
                 listed == 1 ? "" : "s");
    MnColor background = win.frame_background;
    drive_answer("  Window.background = Color(%u, %u, %u, %u)",
                 (unsigned)background.r, (unsigned)background.g,
                 (unsigned)background.b, (unsigned)background.a);
    for (size_t i = 0; i < frame->len; i++) {
        const MnDraw *draw = &frame->data[i];
        if (draw->kind == MN_DRAW_NOTHING || draw->kind > MN_DRAW_TRIANGLE) continue;
        char line[320];
        int len = snprintf(line, sizeof line, "  Draw.%s(", shapes[draw->kind]);
        for (int n = 0; n < numbers[draw->kind]; n++) {
            len += format_number((double)draw->n[n], true, line + len);
            line[len++] = ',';
            line[len++] = ' ';
        }
        snprintf(line + len, sizeof line - (size_t)len, "Color(%u, %u, %u, %u))",
                 (unsigned)draw->color.r, (unsigned)draw->color.g,
                 (unsigned)draw->color.b, (unsigned)draw->color.a);
        drive_answer("%s", line);
    }
}

/* Carries out one command, given without its line break and without the
 * spaces around it. Returns true when it lets ticks run. */
static bool drive_command(char *text) {
    if (strncmp(text, "capture", 7) == 0 &&
        (text[7] == ' ' || text[7] == '\t' || text[7] == '\0')) {
        const char *file = text + 7;
        while (*file == ' ' || *file == '\t') file++;
        window_capture(text, file);
        return false;
    }
    char *copy = mn_malloc(strlen(text) + 1);
    strcpy(copy, text);
    char *words[5];
    int count = drive_words(copy, words, 4);
    const char *name = words[0];
    bool runs = false;
    if (strcmp(name, "tick") == 0) {
        int64_t ticks = 1;
        if (count == 2) ticks = drive_count(words[1], INT64_MAX / 16);
        if (count > 2 || ticks < 1) {
            drive_fail("`tick` takes a number of ticks from 1 up, or nothing");
        }
        free(win.tick_text);
        win.tick_text = mn_malloc(strlen(text) + 1);
        strcpy(win.tick_text, text);
        win.ticks_left = ticks;
        win.ticks_run = 0;
        runs = true;
    } else if (strcmp(name, "resize") == 0 || strcmp(name, "scale") == 0) {
        if (!win.headless) {
            drive_fail("`%s` needs MIDNIGHT_WINDOW=headless", name);
        }
        if (name[0] == 'r') {
            int64_t width = count == 3 ? drive_count(words[1], MN_WINDOW_LARGEST) : -1;
            int64_t height = count == 3 ? drive_count(words[2], MN_WINDOW_LARGEST) : -1;
            if (width < 1 || height < 1) {
                drive_fail("`resize` takes a width and a height in points, "
                           "each from 1 to %d",
                           MN_WINDOW_LARGEST);
            }
            win.width = width;
            win.height = height;
        } else {
            /* A decimal number: digits, then optionally a point and at
             * most six digits more. */
            int64_t num = 0;
            int64_t den = 1;
            bool point = false;
            bool digits = false;
            bool bad = count != 2;
            for (const char *c = bad ? "" : words[1]; *c != '\0' && !bad; c++) {
                if (*c == '.' && !point) {
                    point = true;
                } else if (*c >= '0' && *c <= '9' && num < 100000000 &&
                           den < 1000000) {
                    num = num * 10 + (*c - '0');
                    if (point) den *= 10;
                    digits = true;
                } else {
                    bad = true;
                }
            }
            if (bad || !digits || num * 2 < den || num > den * 4) {
                drive_fail("`scale` takes a number from 0.5 to 4");
            }
            win.scale_num = num;
            win.scale_den = den;
        }
        if (win.open && (scaled(win.width) > MN_WINDOW_LARGEST ||
                         scaled(win.height) > MN_WINDOW_LARGEST)) {
            drive_fail("a frame of %lldx%lld pixels is larger than %d by %d",
                       (long long)scaled(win.width),
                       (long long)scaled(win.height), MN_WINDOW_LARGEST,
                       MN_WINDOW_LARGEST);
        }
        win.pixel_width = scaled(win.width);
        win.pixel_height = scaled(win.height);
        drive_answer("%s", text);
    } else if (strcmp(name, "draws") == 0) {
        if (count != 1) drive_fail("`draws` takes nothing");
        window_draws();
    } else if (strcmp(name, "close") == 0) {
        if (count != 1) drive_fail("`close` takes nothing");
        if (win.open) {
            win.closing = true;
            input_happened(HAPPENED_CLOSE, 0, 0.0, 0.0);
        }
        input.commands.focused = false;
        input.commands.left = true;
        memset(input.commands.keys, 0, sizeof input.commands.keys);
        memset(input.commands.buttons, 0, sizeof input.commands.buttons);
        drive_answer("%s", text);
    } else if (drive_input(text, words, count)) {
    } else {
        drive_fail("unknown command `%s`", name);
    }
    free(copy);
    return runs;
}

/* Reads and carries out commands until one lets ticks run, or until
 * `until_tick` is false and the commands run out. Returns false when they
 * have run out. */
static bool drive_until_tick(void) {
    if (win.drive_done) return false;
    if (!win.drive) {
        win.drive = fopen(win.drive_name, "r");
        if (!win.drive) {
            fflush(stdout);
            fprintf(stderr, "midnight: drive: can't open %s: %s\n",
                    win.drive_name, strerror(errno));
            mn_leave(1);
        }
    }
    for (;;) {
        ssize_t got = getline(&win.line, &win.line_capacity, win.drive);
        if (got < 0) {
            win.drive_done = true;
            return false;
        }
        win.drive_line++;
        char *text = win.line;
        size_t len = (size_t)got;
        while (len > 0 && (text[len - 1] == '\n' || text[len - 1] == '\r' ||
                           text[len - 1] == ' ' || text[len - 1] == '\t')) {
            len--;
        }
        text[len] = '\0';
        while (*text == ' ' || *text == '\t') text++;
        if (*text == '\0' || *text == '#') continue;
        if (drive_command(text)) return true;
    }
}

/* Answers the `tick` command when its last tick has run, or with the
 * number that ran when the program ended first. */
static void drive_tick_answer(bool ending) {
    if (!win.tick_text) return;
    if (win.ticks_left == 0) {
        drive_answer("%s", win.tick_text);
    } else if (ending) {
        drive_answer("%s: the program ended after %lld tick%s", win.tick_text,
                     (long long)win.ticks_run, win.ticks_run == 1 ? "" : "s");
    } else {
        return;
    }
    free(win.tick_text);
    win.tick_text = NULL;
}

/* Makes the frame of the window as the resource has it now, with the draws
 * since the last frame, and begins a new list. */
static void window_frame(void) {
    const MnWindow *window = mn_window_program->window;
    win.frame_width = (uint32_t)window->pixel_width;
    win.frame_height = (uint32_t)window->pixel_height;
    win.frame_point_width = window->width;
    win.frame_point_height = window->height;
    win.frame_background = window->background;
    win.frame_tick = mn_time.tick;
    MnDraws frame = drawing.frame;
    drawing.frame = drawing.list;
    drawing.list = frame;
    drawing.list.len = 0;
}

/* Shows a window of win.width by win.height points titled `title` and
 * sets the sizes the desktop gave it; or returns why it can't be shown. */
static const char *window_show(MnString title, const char **how) {
#ifndef MN_FRAMES
    (void)title;
    (void)how;
    return "the program was built without the window flags";
#else
    if (gpu_options.off) return "MIDNIGHT_GPU has `off`";
    int64_t len = mn_str_bytesize(title);
    char *text = mn_malloc((size_t)len + 1);
    if (len > 0) memcpy(text, title->bytes, (size_t)len);
    text[len] = '\0';
    MnWindowSizes sizes;
    const char *unable = mn_window_show(text, win.width, win.height, &sizes, how);
    free(text);
    if (unable) return unable;
    win.shown = true;
    win.width = sizes.width;
    win.height = sizes.height;
    win.pixel_width = sizes.pixel_width;
    win.pixel_height = sizes.pixel_height;
    return NULL;
#endif
}

#ifdef MN_FRAMES
/* Draws the last frame and shows it on the desktop. */
static void window_draw(void) {
    MnFrame frame = {win.frame_width, win.frame_height, win.frame_background};
    mn_window_draw(&frame);
}
#endif

/* Takes a shown window off the desktop. */
static void window_hide(void) {
#ifdef MN_FRAMES
    if (win.shown) mn_window_close();
#endif
    win.shown = false;
}

void mn_window_open(MnString title, int64_t width, int64_t height, MnLoc loc) {
    if (mn_world.in_tick) {
        mn_panic(loc, "Window.open called while a tick is in progress");
    }
    if (mn_world.defer_depth != 0) {
        mn_panic(loc, "Window.open called inside an `each`");
    }
    if (win.open) mn_panic(loc, "Window.open called while a window is open");
    const char *names[2] = {"width", "height"};
    int64_t sizes[2] = {width, height};
    for (int i = 0; i < 2; i++) {
        if (sizes[i] < 1) {
            mn_panic(loc, "Window.open: a %s of %lld is less than 1", names[i],
                     (long long)sizes[i]);
        }
        if (sizes[i] > MN_WINDOW_LARGEST) {
            mn_panic(loc, "Window.open: a %s of %lld is more than %d", names[i],
                     (long long)sizes[i], MN_WINDOW_LARGEST);
        }
    }
    win.scale_num = 1;
    win.scale_den = 1;
    win.width = width;
    win.height = height;
    win.pixel_width = width;
    win.pixel_height = height;
    const char *how = "headless";
    if (!win.headless) {
        const char *unable = window_show(title, &how);
        if (unable) {
            mn_panic(loc,
                     "Window.open: no window can be shown: %s; set "
                     "MIDNIGHT_WINDOW=headless to run without one",
                     unable);
        }
    }
    win.open = true;
    win.closing = false;
    win.paced = false;
    input.count = 0;
    input_opened(&input.now);
    input_opened(&input.commands);
    input.desktop_left = true;
    if (win.headless || win.drive_name) {
        input.commands.focused = true;
        input_happened(HAPPENED_FOCUS, 0, 0.0, 0.0);
    }
    MnWindow *window = mn_window_program->window;
    window->width = win.width;
    window->height = win.height;
    window->pixel_width = win.pixel_width;
    window->pixel_height = win.pixel_height;
    window->open = true;
    drawing.list.len = 0;
    window_frame();
#ifdef MN_FRAMES
    if (win.shown) window_draw();
#endif
    if (win.info) {
        fflush(stdout);
        fprintf(stderr,
                "midnight: window: %lldx%lld points, %lldx%lld pixels, %s\n",
                (long long)win.width, (long long)win.height,
                (long long)win.pixel_width, (long long)win.pixel_height, how);
        if (win.drive_name) fputs("midnight: window: input from commands\n", stderr);
    }
}

#ifdef MN_FRAMES
/* Holds a tick of a shown window that is not driven until `Time.dt` after
 * the tick before it was due. A tick that begins late is due when it
 * begins; the first after Window.open begins at once. */
static void window_pace(void) {
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    int64_t now_ns = (int64_t)now.tv_sec * 1000000000 + now.tv_nsec;
    double dt = mn_time.dt;
    if (!win.paced || !(dt > 0.0)) {
        win.paced = true;
        win.due = now_ns;
        return;
    }
    win.due += dt < 1e9 ? (int64_t)(dt * 1e9) : INT64_C(1000000000000000000);
    if (win.due <= now_ns) {
        win.due = now_ns;
        return;
    }
    fflush(stdout);
    struct timespec due = {(time_t)(win.due / 1000000000), (long)(win.due % 1000000000)};
    while (clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &due, NULL) == EINTR) {
    }
}
#endif

/* What happens before the first system of a tick runs. */
static void window_tick_begin(void) {
    input_clear();
    if (win.open && win.drive_name && win.ticks_left == 0 &&
        !drive_until_tick()) {
        win.closing = true;
    }
    if (win.ticks_left > 0) {
        win.ticks_left--;
        win.ticks_run++;
    }
    if (!win.open) {
        input.count = 0;
        input_assign();
        return;
    }
    if (win.drive_done) win.closing = true;
#ifdef MN_FRAMES
    if (win.shown) {
        if (!win.drive_name) window_pace();
        MnWindowSizes sizes;
        mn_window_poll(&sizes);
        win.width = sizes.width;
        win.height = sizes.height;
        win.pixel_width = sizes.pixel_width;
        win.pixel_height = sizes.pixel_height;
        if (sizes.closed) win.closing = true;
    }
#endif
    MnWindow *window = mn_window_program->window;
    if (window->width != win.width || window->height != win.height ||
        window->pixel_width != win.pixel_width ||
        window->pixel_height != win.pixel_height) {
        window->width = win.width;
        window->height = win.height;
        window->pixel_width = win.pixel_width;
        window->pixel_height = win.pixel_height;
        MnWindowResized *event = mn_emit(mn_window_program->resized);
        event->width = win.width;
        event->height = win.height;
        event->pixel_width = win.pixel_width;
        event->pixel_height = win.pixel_height;
    }
    input_apply();
    if (win.closing) {
        input_close();
        input_opened(&input.commands);
    }
    input_assign();
    if (win.closing) {
        win.open = false;
        win.closing = false;
        window->open = false;
        window_hide();
        drawing.list.len = 0;
        drawing.frame.len = 0;
        mn_emit(mn_window_program->closed);
        mn_stop();
    }
}

/* What happens when a tick has ended. */
static void window_tick_end(void) {
    if (win.open) {
        window_frame();
#ifdef MN_FRAMES
        if (win.shown) window_draw();
#endif
    }
    drive_tick_answer(false);
}

#ifdef MN_FRAMES
/* ---- PNG ---- */

/* The CRC-32 of PNG and zlib's gzip, continued from `crc` over `len`
 * bytes; 0 to begin. */
static uint32_t crc32_of(uint32_t crc, const unsigned char *bytes, size_t len) {
    static uint32_t table[256];
    if (table[1] == 0) {
        for (uint32_t n = 0; n < 256; n++) {
            uint32_t c = n;
            for (int k = 0; k < 8; k++) c = c & 1 ? 0xedb88320u ^ (c >> 1) : c >> 1;
            table[n] = c;
        }
    }
    crc = ~crc;
    for (size_t i = 0; i < len; i++) crc = table[(crc ^ bytes[i]) & 0xff] ^ (crc >> 8);
    return ~crc;
}

/* Bytes written a bit at a time, the first bit in the lowest bit of a
 * byte, as deflate writes them. */
typedef struct PngOut {
    unsigned char *data;
    size_t len;
    size_t capacity;
    uint64_t bits;
    int count;
} PngOut;

static void png_byte(PngOut *out, unsigned char byte) {
    if (out->len == out->capacity) {
        out->capacity = out->capacity ? out->capacity * 2 : 4096;
        out->data = mn_realloc(out->data, out->capacity);
    }
    out->data[out->len++] = byte;
}

static void png_u32(PngOut *out, uint32_t value) {
    for (int shift = 24; shift >= 0; shift -= 8) png_byte(out, (unsigned char)(value >> shift));
}

/* Writes the `n` low bits of `value`, lowest first. */
static void png_bits(PngOut *out, uint32_t value, int n) {
    out->bits |= (uint64_t)value << out->count;
    out->count += n;
    while (out->count >= 8) {
        png_byte(out, (unsigned char)out->bits);
        out->bits >>= 8;
        out->count -= 8;
    }
}

/* Writes a Huffman code of `n` bits, its highest bit first. */
static void png_code(PngOut *out, uint32_t code, int n) {
    uint32_t reversed = 0;
    for (int i = 0; i < n; i++) reversed |= ((code >> i) & 1u) << (n - 1 - i);
    png_bits(out, reversed, n);
}

/* A symbol of deflate's fixed literal and length code. */
static void png_symbol(PngOut *out, uint32_t symbol) {
    if (symbol < 144) {
        png_code(out, 0x30 + symbol, 8);
    } else if (symbol < 256) {
        png_code(out, 0x190 + symbol - 144, 9);
    } else if (symbol < 280) {
        png_code(out, symbol - 256, 7);
    } else {
        png_code(out, 0xc0 + symbol - 280, 8);
    }
}

/* A copy of `length` bytes, 3 to 258, from `distance` bytes back, 1 to
 * 32768. */
static void png_match(PngOut *out, uint32_t length, uint32_t distance) {
    static const uint16_t length_base[29] = {
        3,  4,  5,  6,  7,  8,  9,  10, 11,  13,  15,  17,  19,  23, 27,
        31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258};
    static const uint8_t length_extra[29] = {0, 0, 0, 0, 0, 0, 0, 0, 1, 1,
                                             1, 1, 2, 2, 2, 2, 3, 3, 3, 3,
                                             4, 4, 4, 4, 5, 5, 5, 5, 0};
    static const uint16_t distance_base[30] = {
        1,    2,    3,    4,    5,    7,     9,     13,    17,  25,
        33,   49,   65,   97,   129,  193,   257,   385,   513, 769,
        1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577};
    static const uint8_t distance_extra[30] = {0, 0, 0, 0, 1, 1, 2,  2,  3,  3,
                                               4, 4, 5, 5, 6, 6, 7,  7,  8,  8,
                                               9, 9, 10, 10, 11, 11, 12, 12, 13, 13};
    int l = 28;
    while (length_base[l] > length) l--;
    png_symbol(out, 257 + (uint32_t)l);
    png_bits(out, length - length_base[l], length_extra[l]);
    int d = 29;
    while (distance_base[d] > distance) d--;
    png_code(out, (uint32_t)d, 5);
    png_bits(out, distance - distance_base[d], distance_extra[d]);
}

/* How many bytes from `at`, at most 258, equal those `distance` bytes
 * before them. */
static uint32_t png_run(const unsigned char *raw, size_t at, size_t end,
                        size_t distance) {
    if (distance == 0 || distance > 32768 || at < distance) return 0;
    uint32_t length = 0;
    while (length < 258 && at + length < end &&
           raw[at + length] == raw[at + length - distance]) {
        length++;
    }
    return length;
}

/* Writes a chunk of the PNG from `out->data[start]` on: its length and
 * type were written before the data, and its CRC follows. */
static void png_chunk_end(PngOut *out, size_t start) {
    size_t length = out->len - start - 4;
    for (int i = 0; i < 4; i++) {
        out->data[start - 4 + i] = (unsigned char)(length >> (24 - 8 * i));
    }
    png_u32(out, crc32_of(0, out->data + start, out->len - start));
}

static size_t png_chunk_begin(PngOut *out, const char *type) {
    png_u32(out, 0);
    size_t start = out->len;
    for (int i = 0; i < 4; i++) png_byte(out, (unsigned char)type[i]);
    return start;
}

/* The PNG file of `rgba`, 8 bits each of red, green, blue and alpha, in
 * `out->data`. Every row is stored unfiltered, and the pixels are one
 * deflate block of fixed codes in which a run of bytes equal to those of
 * the pixel before, or of the pixel above, is a copy. */
static void png_make(PngOut *out, uint32_t width, uint32_t height,
                     const unsigned char *rgba) {
    static const unsigned char signature[8] = {0x89, 'P', 'N', 'G', '\r', '\n', 0x1a, '\n'};
    for (int i = 0; i < 8; i++) png_byte(out, signature[i]);

    size_t start = png_chunk_begin(out, "IHDR");
    png_u32(out, width);
    png_u32(out, height);
    png_byte(out, 8); /* bits a sample */
    png_byte(out, 6); /* red, green, blue and alpha */
    png_byte(out, 0);
    png_byte(out, 0);
    png_byte(out, 0);
    png_chunk_end(out, start);

    size_t stride = 1 + (size_t)width * 4;
    size_t end = stride * height;
    unsigned char *raw = mn_malloc(end);
    for (uint32_t y = 0; y < height; y++) {
        raw[y * stride] = 0;
        memcpy(raw + y * stride + 1, rgba + (size_t)y * width * 4, (size_t)width * 4);
    }
    start = png_chunk_begin(out, "IDAT");
    png_byte(out, 0x78);
    png_byte(out, 0x01);
    png_bits(out, 1, 1); /* the last block */
    png_bits(out, 1, 2); /* of fixed codes */
    size_t at = 0;
    while (at < end) {
        uint32_t left = png_run(raw, at, end, 4);
        uint32_t up = png_run(raw, at, end, stride);
        uint32_t length = left >= up ? left : up;
        if (length >= 3) {
            png_match(out, length, left >= up ? 4 : (uint32_t)stride);
            at += length;
        } else {
            png_symbol(out, raw[at++]);
        }
    }
    png_symbol(out, 256);
    if (out->count > 0) png_bits(out, 0, 8 - out->count);
    uint32_t a = 1;
    uint32_t b = 0;
    for (size_t i = 0; i < end; i++) {
        a = (a + raw[i]) % 65521;
        b = (b + a) % 65521;
    }
    png_u32(out, (b << 16) | a);
    free(raw);
    png_chunk_end(out, start);

    start = png_chunk_begin(out, "IEND");
    png_chunk_end(out, start);
}
#endif

/* Writes the last frame to `file`, for `capture`, and answers with the
 * number of ticks it is the picture of, its size and the CRC-32 of its
 * pixels. */
static void window_capture(const char *text, const char *file) {
    if (*file == '\0') drive_fail("`capture` takes the name of a file");
    if (!win.open) drive_fail("can't capture: no window is open");
#ifndef MN_FRAMES
    (void)text;
    drive_fail("can't capture: the program was built without the window flags");
#else
    MnFrame frame = {win.frame_width, win.frame_height, win.frame_background};
    size_t size = (size_t)frame.width * frame.height * 4;
    unsigned char *rgba = mn_malloc(size);
    const char *unable = mn_window_read(&frame, rgba);
    if (unable) drive_fail("can't capture: %s", unable);
    PngOut png = {0};
    png_make(&png, frame.width, frame.height, rgba);
    uint32_t crc = crc32_of(0, rgba, size);
    free(rgba);
    FILE *out = fopen(file, "wb");
    bool written = out && fwrite(png.data, 1, png.len, out) == png.len;
    if (out && fclose(out) != 0) written = false;
    free(png.data);
    if (!written) {
        drive_fail("can't capture: can't write %s: %s", file, strerror(errno));
    }
    drive_answer("%s: %lld tick%s, %ux%u, crc %08x", text, (long long)win.frame_tick,
                 win.frame_tick == 1 ? "" : "s", frame.width, frame.height,
                 (unsigned)crc);
#endif
}

/* When the program ends: answers the `tick` command, carries out the
 * commands up to the next `tick`, and closes the window. */
static void window_finish(void) {
    drive_tick_answer(true);
    if (win.drive_name && !win.drive_done) drive_until_tick();
    free(win.tick_text);
    win.tick_text = NULL;
    free(win.line);
    win.line = NULL;
    free(input.list);
    input.list = NULL;
    input.count = 0;
    input.capacity = 0;
    if (win.drive) fclose(win.drive);
    win.drive = NULL;
    win.drive_done = true;
    win.open = false;
    free(drawing.list.data);
    free(drawing.frame.data);
    memset(&drawing, 0, sizeof drawing);
    window_hide();
#ifdef MN_FRAMES
    mn_window_finish();
#endif
}
#endif

/* ---- Systems and ticks --------------------------------------------------- */

#ifdef MN_GPU_SYSTEMS
static void gpu_tick_end(void);
#endif

static void tick(void) {
    MnWorld *w = &mn_world;
    const MnProgram *program = w->program;
#ifdef MN_WINDOW
    window_tick_begin();
#endif
    gpu_stats_line();
    w->in_tick = true;
    for (uint32_t i = 0; i < program->system_count; i++) program->systems[i]();
    w->in_tick = false;
#ifdef MN_GPU_SYSTEMS
    gpu_tick_end();
#endif
    mn_time.tick++;
    mn_time.elapsed += mn_time.dt;
    for (uint32_t i = 0; i < program->event_count; i++) {
        events_discard(i, w->events[i].mark);
        w->events[i].mark = w->events[i].end;
    }
#ifdef MN_WINDOW
    window_tick_end();
#endif
}

void mn_tick(MnLoc loc) {
    if (mn_world.in_tick) {
        mn_panic(loc, "World.tick called while a tick is in progress");
    }
    if (mn_world.defer_depth != 0) {
        mn_panic(loc, "World.tick called inside an `each`");
    }
    tick();
}

void mn_run(MnLoc loc) {
    if (mn_world.in_tick) {
        mn_panic(loc, "World.run called while a tick is in progress");
    }
    if (mn_world.defer_depth != 0) {
        mn_panic(loc, "World.run called inside an `each`");
    }
    mn_world.stop_requested = false;
    do {
        tick();
    } while (!mn_world.stop_requested);
}

void mn_stop(void) { mn_world.stop_requested = true; }

/* ---- Text forms ---------------------------------------------------------- */

/* Each writes at `out` and returns the number of bytes written. */

enum { MN_INT_TEXT = 20, MN_FLOAT_TEXT = 32, MN_ENTITY_TEXT = 32 };

static int format_int(int64_t value, char *out) {
    char digits[20];
    int count = 0;
    uint64_t rest = value < 0 ? 0 - (uint64_t)value : (uint64_t)value;
    do {
        digits[count++] = (char)('0' + rest % 10);
        rest /= 10;
    } while (rest != 0);
    int len = 0;
    if (value < 0) out[len++] = '-';
    while (count > 0) out[len++] = digits[--count];
    return len;
}

/* Writes `value` with `precision` significant digits in exponent form and
 * reports whether that text reads back as the same number: as the same
 * 32-bit float when `single`. */
static bool float_reads_back(double value, bool single, int precision,
                             char *text, size_t size) {
    snprintf(text, size, "%.*e", precision - 1, value);
    if (single) return strtof(text, NULL) == (float)value;
    return strtod(text, NULL) == value;
}

/* The text of a `Float`; with `single`, of a 32-bit float, with the
 * fewest digits that read back as that float. */
static int format_number(double value, bool single, char *out) {
    if (value != value) {
        memcpy(out, "NaN", 3);
        return 3;
    }
    if (value - value != 0.0) {
        if (value < 0) {
            memcpy(out, "-Infinity", 9);
            return 9;
        }
        memcpy(out, "Infinity", 8);
        return 8;
    }

    /* The fewest significant digits that read back. When 15 digits do not,
     * no smaller number does; 9 always do for a 32-bit float. */
    char text[32];
    int most = single ? 9 : 17;
    int precision =
        single || float_reads_back(value, false, 15, text, sizeof text) ? 1 : 16;
    while (precision < most &&
           !float_reads_back(value, single, precision, text, sizeof text)) {
        precision++;
    }
    if (precision == most) snprintf(text, sizeof text, "%.*e", most - 1, value);

    /* `text` is [-]d[.ddd]e[+-]xx: split it into digits and exponent. */
    const char *in = text;
    int len = 0;
    if (*in == '-') {
        out[len++] = '-';
        in++;
    }
    char digits[20];
    int count = 0;
    for (; *in != 'e' && *in != '\0'; in++) {
        if (*in >= '0' && *in <= '9' && count < 17) digits[count++] = *in;
    }
    int exponent = *in == 'e' ? atoi(in + 1) : 0;

    if (exponent >= -5 && exponent < 16) {
        if (exponent >= 0) {
            for (int i = 0; i <= exponent; i++) {
                out[len++] = i < count ? digits[i] : '0';
            }
            out[len++] = '.';
            if (count <= exponent + 1) out[len++] = '0';
            for (int i = exponent + 1; i < count; i++) out[len++] = digits[i];
        } else {
            out[len++] = '0';
            out[len++] = '.';
            for (int i = -1; i > exponent; i--) out[len++] = '0';
            for (int i = 0; i < count; i++) out[len++] = digits[i];
        }
        return len;
    }

    out[len++] = digits[0];
    if (count > 1) {
        out[len++] = '.';
        for (int i = 1; i < count; i++) out[len++] = digits[i];
    }
    out[len++] = 'e';
    out[len++] = exponent < 0 ? '-' : '+';
    if (exponent < 0) exponent = -exponent;
    if (exponent < 10) out[len++] = '0';
    return len + format_int(exponent, out + len);
}

static int format_float(double value, char *out) {
    return format_number(value, false, out);
}

static int format_entity(MnEntity e, char *out) {
    if (e.index == 0 && e.generation == 0) {
        memcpy(out, "Entity(none)", 12);
        return 12;
    }
    memcpy(out, "Entity(", 7);
    int len = 7;
    len += format_int(e.index, out + len);
    out[len++] = 'v';
    len += format_int(e.generation, out + len);
    out[len++] = ')';
    return len;
}

/* ---- Failures reported from the header's inline functions --------------- */

_Noreturn void mn_get_failed(MnEntity e, uint32_t component, MnLoc loc) {
    char entity[MN_ENTITY_TEXT];
    int len = format_entity(e, entity);
    if (mn_alive(e)) {
        mn_panic(loc, "%.*s has no %s", len, entity,
                 mn_world.program->components[component].name);
    }
    mn_panic(loc, "%.*s is not alive", len, entity);
}

_Noreturn void mn_index_failed(int64_t index, int64_t len, MnLoc loc) {
    mn_panic(loc, "index %lld out of range for a list of length %lld",
             (long long)index, (long long)len);
}

_Noreturn void mn_div_failed(const char *what, MnLoc loc) {
    mn_panic(loc, "%s by zero", what);
}

_Noreturn void mn_range_failed(int64_t value, const char *type, MnLoc loc) {
    mn_panic(loc, "%lld does not fit in %s", (long long)value, type);
}

/* ---- Strings ------------------------------------------------------------- */

/* A new buffer for `len` bytes (above zero), which the caller fills in.
 * A NUL follows the bytes. */
static MnString str_alloc(int64_t len) {
    if ((uint64_t)len > SIZE_MAX - sizeof(MnStr) - 1) mn_out_of_memory();
    MnStr *s = mn_malloc(sizeof(MnStr) + (size_t)len + 1);
    s->refs = 1;
    s->len = len;
    s->bytes[len] = '\0';
    MN_BUFFER_BORN();
    return s;
}

void mn_str_free(MnString s) {
    if (!s || s->refs < 0) return;
    free(s);
    MN_BUFFER_GONE();
}

MnString mn_str_new(const char *bytes, int64_t len) {
    if (len <= 0) return NULL;
    MnString s = str_alloc(len);
    memcpy(s->bytes, bytes, (size_t)len);
    return s;
}

MnString mn_str_concat(MnString a, MnString b) {
    if (!a) {
        mn_str_retain(b);
        return b;
    }
    if (!b) {
        mn_str_retain(a);
        return a;
    }
    if (a->len > INT64_MAX - b->len) mn_out_of_memory();
    MnString s = str_alloc(a->len + b->len);
    memcpy(s->bytes, a->bytes, (size_t)a->len);
    memcpy(s->bytes + a->len, b->bytes, (size_t)b->len);
    return s;
}

MnString mn_str_repeat(MnString s, int64_t n) {
    if (!s || n <= 0) return NULL;
    if (n == 1) {
        mn_str_retain(s);
        return s;
    }
    if (s->len > INT64_MAX / n) mn_out_of_memory();
    MnString result = str_alloc(s->len * n);
    for (int64_t i = 0; i < n; i++) {
        memcpy(result->bytes + i * s->len, s->bytes, (size_t)s->len);
    }
    return result;
}

bool mn_str_eq(MnString a, MnString b) {
    if (a == b) return true;
    int64_t len = mn_str_bytesize(a);
    if (len != mn_str_bytesize(b)) return false;
    return len == 0 || memcmp(a->bytes, b->bytes, (size_t)len) == 0;
}

int mn_str_cmp(MnString a, MnString b) {
    int64_t a_len = mn_str_bytesize(a);
    int64_t b_len = mn_str_bytesize(b);
    int64_t shared = a_len < b_len ? a_len : b_len;
    if (shared != 0) {
        int order = memcmp(a->bytes, b->bytes, (size_t)shared);
        if (order != 0) return order < 0 ? -1 : 1;
    }
    return (a_len > b_len) - (a_len < b_len);
}

int64_t mn_str_length(MnString s) {
    if (!s) return 0;
    int64_t count = 0;
    for (int64_t i = 0; i < s->len; i++) {
        count += ((unsigned char)s->bytes[i] & 0xc0) != 0x80;
    }
    return count;
}

/* Copies `s` with the ASCII letters in [first, first + 26) moved by
 * `shift`. Returns `s` itself when it has none. */
static MnString str_shift_case(MnString s, char first, int shift) {
    if (!s) return NULL;
    int64_t i = 0;
    while (i < s->len && !(s->bytes[i] >= first && s->bytes[i] < first + 26)) {
        i++;
    }
    if (i == s->len) {
        mn_str_retain(s);
        return s;
    }
    MnString result = str_alloc(s->len);
    for (i = 0; i < s->len; i++) {
        char byte = s->bytes[i];
        if (byte >= first && byte < first + 26) byte = (char)(byte + shift);
        result->bytes[i] = byte;
    }
    return result;
}

MnString mn_str_upcase(MnString s) { return str_shift_case(s, 'a', 'A' - 'a'); }

MnString mn_str_downcase(MnString s) {
    return str_shift_case(s, 'A', 'a' - 'A');
}

bool mn_str_include(MnString s, MnString part) {
    if (!part) return true;
    if (!s || part->len > s->len) return false;
    size_t len = (size_t)part->len;
    const char *at = s->bytes;
    const char *last = s->bytes + (s->len - part->len);
    while (at <= last) {
        at = memchr(at, part->bytes[0], (size_t)(last - at) + 1);
        if (!at) return false;
        if (memcmp(at, part->bytes, len) == 0) return true;
        at++;
    }
    return false;
}

bool mn_str_start_with(MnString s, MnString prefix) {
    if (!prefix) return true;
    if (!s || prefix->len > s->len) return false;
    return memcmp(s->bytes, prefix->bytes, (size_t)prefix->len) == 0;
}

bool mn_str_end_with(MnString s, MnString suffix) {
    if (!suffix) return true;
    if (!s || suffix->len > s->len) return false;
    return memcmp(s->bytes + (s->len - suffix->len), suffix->bytes,
                  (size_t)suffix->len) == 0;
}

/* `s` with spaces after it (`after`) or before it, up to `width` code
 * points. */
static MnString str_pad(MnString s, int64_t width, bool after) {
    int64_t length = mn_str_length(s);
    if (width <= length) {
        mn_str_retain(s);
        return s;
    }
    int64_t pad = width - length;
    int64_t len = mn_str_bytesize(s);
    if (pad > INT64_MAX - len) mn_out_of_memory();
    MnString result = str_alloc(len + pad);
    char *text = result->bytes + (after ? 0 : pad);
    char *spaces = result->bytes + (after ? len : 0);
    if (len != 0) memcpy(text, s->bytes, (size_t)len);
    memset(spaces, ' ', (size_t)pad);
    return result;
}

MnString mn_str_ljust(MnString s, int64_t width) {
    return str_pad(s, width, true);
}

MnString mn_str_rjust(MnString s, int64_t width) {
    return str_pad(s, width, false);
}

static bool is_space(char c) {
    return c == ' ' || (c >= '\t' && c <= '\r');
}

static bool is_digit(char c) { return c >= '0' && c <= '9'; }

int64_t mn_str_to_i(MnString s) {
    if (!s) return 0;
    /* The NUL after the bytes ends every scan. */
    const char *at = s->bytes;
    while (is_space(*at)) at++;
    bool negative = *at == '-';
    if (*at == '-' || *at == '+') at++;
    uint64_t value = 0;
    while (is_digit(*at)) {
        value = value * 10 + (uint64_t)(*at - '0');
        at++;
        if (*at == '_' && is_digit(at[1])) at++;
    }
    return (int64_t)(negative ? 0 - value : value);
}

double mn_str_to_f(MnString s) {
    if (!s) return 0.0;
    const char *at = s->bytes;
    while (is_space(*at)) at++;
    const char *start = at;
    bool negative = *at == '-';
    if (*at == '-' || *at == '+') at++;
    if (!is_digit(*at) && !(*at == '.' && is_digit(at[1]))) return 0.0;
    if (*at == '0' && (at[1] == 'x' || at[1] == 'X')) {
        return negative ? -0.0 : 0.0;
    }
    return strtod(start, NULL);
}

MnString mn_int_to_s(int64_t value) {
    char text[MN_INT_TEXT];
    return mn_str_new(text, format_int(value, text));
}

MnString mn_float_to_s(double value) {
    char text[MN_FLOAT_TEXT];
    return mn_str_new(text, format_float(value, text));
}

MN_STR_LIT(mn_text_true, 4, "true");
MN_STR_LIT(mn_text_false, 5, "false");

MnString mn_bool_to_s(bool value) {
    return value ? (MnString)(void *)&mn_text_true
                 : (MnString)(void *)&mn_text_false;
}

MnString mn_entity_to_s(MnEntity e) {
    char text[MN_ENTITY_TEXT];
    return mn_str_new(text, format_entity(e, text));
}

/* ---- String builder ------------------------------------------------------ */

/* Makes room for `extra` more bytes and returns where they go. */
static char *sb_reserve(MnBuilder *sb, int64_t extra) {
    if (sb->capacity - sb->len < extra) {
        int64_t capacity = sb->capacity ? sb->capacity : 64;
        while (capacity - sb->len < extra) {
            if (capacity > INT64_MAX / 2) mn_out_of_memory();
            capacity *= 2;
        }
        sb->bytes = mn_realloc(sb->bytes, (size_t)capacity);
        sb->capacity = capacity;
    }
    return sb->bytes + sb->len;
}

void mn_sb_bytes(MnBuilder *sb, const char *bytes, int64_t len) {
    if (len <= 0) return;
    memcpy(sb_reserve(sb, len), bytes, (size_t)len);
    sb->len += len;
}

void mn_sb_str(MnBuilder *sb, MnString s) {
    if (s) mn_sb_bytes(sb, s->bytes, s->len);
}

void mn_sb_quoted(MnBuilder *sb, MnString s) {
    static const char hex[] = "0123456789ABCDEF";
    int64_t len = mn_str_bytesize(s);
    mn_sb_bytes(sb, "\"", 1);
    int64_t plain = 0; /* start of the bytes not yet appended */
    for (int64_t i = 0; i < len; i++) {
        unsigned char byte = (unsigned char)s->bytes[i];
        char escape[4] = {'\\', 0, 0, 0};
        int escape_len = 2;
        switch (byte) {
        case '\n': escape[1] = 'n'; break;
        case '\t': escape[1] = 't'; break;
        case '\r': escape[1] = 'r'; break;
        case '\0': escape[1] = '0'; break;
        case 0x1b: escape[1] = 'e'; break;
        case '\\': escape[1] = '\\'; break;
        case '"': escape[1] = '"'; break;
        case '#':
            if (i + 1 >= len || s->bytes[i + 1] != '{') continue;
            escape[1] = '#';
            break;
        default:
            if (byte >= 0x20) continue;
            escape[1] = 'x';
            escape[2] = hex[byte >> 4];
            escape[3] = hex[byte & 15];
            escape_len = 4;
            break;
        }
        mn_sb_bytes(sb, s->bytes + plain, i - plain);
        mn_sb_bytes(sb, escape, escape_len);
        plain = i + 1;
    }
    if (len != 0) mn_sb_bytes(sb, s->bytes + plain, len - plain);
    mn_sb_bytes(sb, "\"", 1);
}

void mn_sb_int(MnBuilder *sb, int64_t value) {
    sb->len += format_int(value, sb_reserve(sb, MN_INT_TEXT));
}

void mn_sb_float(MnBuilder *sb, double value) {
    sb->len += format_float(value, sb_reserve(sb, MN_FLOAT_TEXT));
}

void mn_sb_bool(MnBuilder *sb, bool value) {
    if (value) {
        mn_sb_bytes(sb, "true", 4);
    } else {
        mn_sb_bytes(sb, "false", 5);
    }
}

void mn_sb_entity(MnBuilder *sb, MnEntity e) {
    sb->len += format_entity(e, sb_reserve(sb, MN_ENTITY_TEXT));
}

static void sb_reset(MnBuilder *sb) {
    free(sb->bytes);
    sb->bytes = NULL;
    sb->len = 0;
    sb->capacity = 0;
}

MnString mn_sb_finish(MnBuilder *sb) {
    MnString s = mn_str_new(sb->bytes, sb->len);
    sb_reset(sb);
    return s;
}

void mn_sb_write(MnBuilder *sb) {
    mn_out_bytes(sb->bytes, sb->len);
    sb_reset(sb);
}

/* ---- Output -------------------------------------------------------------- */

void mn_out_bytes(const char *bytes, int64_t len) {
    if (len > 0) fwrite(bytes, 1, (size_t)len, stdout);
}

void mn_out_str(MnString s) {
    if (s) mn_out_bytes(s->bytes, s->len);
}

void mn_out_int(int64_t value) {
    char text[MN_INT_TEXT];
    mn_out_bytes(text, format_int(value, text));
}

void mn_out_float(double value) {
    char text[MN_FLOAT_TEXT];
    mn_out_bytes(text, format_float(value, text));
}

void mn_out_bool(bool value) {
    if (value) {
        mn_out_bytes("true", 4);
    } else {
        mn_out_bytes("false", 5);
    }
}

void mn_out_entity(MnEntity e) {
    char text[MN_ENTITY_TEXT];
    mn_out_bytes(text, format_entity(e, text));
}

void mn_out_newline(void) { putc('\n', stdout); }

/* ---- Lists --------------------------------------------------------------- */

static char *list_at(MnList list, const MnElem *elem, int64_t index) {
    return (char *)mn_list_data(list) + (size_t)index * elem->size;
}

static size_t list_bytes(const MnElem *elem, int64_t capacity) {
    if ((uint64_t)capacity > SIZE_MAX) mn_out_of_memory();
    size_t bytes = mn_mul((size_t)capacity, elem->size);
    if (bytes > SIZE_MAX - sizeof(MnListBuf)) mn_out_of_memory();
    return sizeof(MnListBuf) + bytes;
}

/* A new buffer with room for `capacity` elements (above zero) and none in
 * use. */
static MnList list_new(const MnElem *elem, int64_t capacity) {
    MnList list = mn_malloc(list_bytes(elem, capacity));
    list->refs = 1;
    list->len = 0;
    list->cap = capacity;
    MN_BUFFER_BORN();
    return list;
}

/* Appends copies of `count` elements, retaining what they own. The buffer
 * must have room. */
static void list_append_copies(MnList list, const MnElem *elem,
                               const char *from, int64_t count) {
    char *to = list_at(list, elem, list->len);
    memcpy(to, from, (size_t)count * elem->size);
    if (elem->retain) {
        for (int64_t i = 0; i < count; i++) {
            elem->retain(to + (size_t)i * elem->size);
        }
    }
    list->len += count;
}

static int64_t list_grown(int64_t capacity) {
    if (capacity > INT64_MAX / 2) mn_out_of_memory();
    return capacity < 2 ? 4 : capacity * 2;
}

void mn_list_free(MnList list, const MnElem *elem) {
    if (!list) return;
    if (elem->drop) {
        for (int64_t i = 0; i < list->len; i++) {
            elem->drop(list_at(list, elem, i));
        }
    }
    free(list);
    MN_BUFFER_GONE();
}

MnList mn_list_alloc(const MnElem *elem, int64_t len) {
    if (len <= 0) return NULL;
    MnList list = list_new(elem, len);
    memset(mn_list_data(list), 0, (size_t)len * elem->size);
    list->len = len;
    return list;
}

void mn_list_unique(MnList *list, const MnElem *elem) {
    MnList shared = *list;
    if (!shared || shared->refs == 1) return;
    MnList own = NULL;
    if (shared->len != 0) {
        own = list_new(elem, shared->len);
        list_append_copies(own, elem, mn_list_data(shared), shared->len);
    }
    shared->refs--;
    *list = own;
}

int64_t mn_list_need(MnList list, const char *what, MnLoc loc) {
    int64_t len = mn_list_len(list);
    if (len == 0) mn_panic(loc, "%s on an empty list", what);
    return len;
}

void *mn_list_push(MnList *list, const MnElem *elem) {
    MnList buf = *list;
    if (!buf) {
        buf = list_new(elem, 4);
    } else if (buf->refs > 1) {
        MnList own = list_new(elem, list_grown(buf->len));
        list_append_copies(own, elem, mn_list_data(buf), buf->len);
        buf->refs--;
        buf = own;
    } else if (buf->len == buf->cap) {
        int64_t capacity = list_grown(buf->cap);
        buf = mn_realloc(buf, list_bytes(elem, capacity));
        buf->cap = capacity;
    }
    *list = buf;
    void *slot = list_at(buf, elem, buf->len);
    memset(slot, 0, elem->size);
    buf->len++;
    return slot;
}

void mn_list_pop(MnList *list, const MnElem *elem, void *out, MnLoc loc) {
    int64_t len = mn_list_need(*list, "pop", loc);
    mn_list_unique(list, elem);
    MnList buf = *list;
    memcpy(out, list_at(buf, elem, len - 1), elem->size);
    buf->len = len - 1;
}

void mn_list_delete_at(MnList *list, const MnElem *elem, int64_t index,
                       void *out, MnLoc loc) {
    int64_t at = mn_list_index(*list, index, loc);
    mn_list_unique(list, elem);
    MnList buf = *list;
    char *slot = list_at(buf, elem, at);
    memcpy(out, slot, elem->size);
    buf->len--;
    memmove(slot, slot + elem->size, (size_t)(buf->len - at) * elem->size);
}

void mn_list_clear(MnList *list, const MnElem *elem) {
    mn_list_release(*list, elem);
    *list = NULL;
}

MnList mn_list_concat(MnList a, MnList b, const MnElem *elem) {
    int64_t a_len = mn_list_len(a);
    int64_t b_len = mn_list_len(b);
    if (a_len == 0 && b_len == 0) return NULL;
    if (b_len == 0) {
        mn_list_retain(a);
        return a;
    }
    if (a_len == 0) {
        mn_list_retain(b);
        return b;
    }
    if (a_len > INT64_MAX - b_len) mn_out_of_memory();
    MnList list = list_new(elem, a_len + b_len);
    list_append_copies(list, elem, mn_list_data(a), a_len);
    list_append_copies(list, elem, mn_list_data(b), b_len);
    return list;
}

MnList mn_list_repeat(MnList list, int64_t n, const MnElem *elem) {
    int64_t len = mn_list_len(list);
    if (len == 0 || n <= 0) return NULL;
    if (n == 1) {
        mn_list_retain(list);
        return list;
    }
    if (len > INT64_MAX / n) mn_out_of_memory();
    MnList result = list_new(elem, len * n);
    for (int64_t i = 0; i < n; i++) {
        list_append_copies(result, elem, mn_list_data(list), len);
    }
    return result;
}

/* ---- Numbers ------------------------------------------------------------- */

int64_t mn_int_pow(int64_t base, int64_t exponent, MnLoc loc) {
    if (exponent < 0) {
        mn_panic(loc, "negative exponent %lld", (long long)exponent);
    }
    uint64_t result = 1;
    uint64_t factor = (uint64_t)base;
    for (uint64_t left = (uint64_t)exponent; left != 0; left >>= 1) {
        if (left & 1) result *= factor;
        factor *= factor;
    }
    return (int64_t)result;
}

double mn_float_mod(double a, double b) {
    if (b == 0.0) return NAN;
    double r = fmod(a, b);
    if (r != 0.0 && (r < 0.0) != (b < 0.0)) r += b;
    return r;
}

/* A Float with no fractional part as an Int. */
static int64_t whole_to_int(double value) {
    if (value != value) return 0;
    if (value >= 9223372036854775808.0) return INT64_MAX;
    if (value <= -9223372036854775808.0) return INT64_MIN;
    return (int64_t)value;
}

int64_t mn_float_to_i(double value) { return whole_to_int(trunc(value)); }
int64_t mn_float_floor(double value) { return whole_to_int(floor(value)); }
int64_t mn_float_ceil(double value) { return whole_to_int(ceil(value)); }
int64_t mn_float_round(double value) { return whole_to_int(round(value)); }

double mn_float_round_digits(double value, int64_t digits) {
    if (value - value != 0.0 || value == 0.0) return value;
    if (digits >= 0) {
        if (digits > 308) return value;
        double scale = pow(10.0, (double)digits);
        double scaled = value * scale;
        /* At or above 2^52 the value has no fractional digits left. */
        if (scaled - scaled != 0.0 || fabs(scaled) >= 4503599627370496.0) {
            return value;
        }
        return round(scaled) / scale;
    }
    if (digits < -308) return copysign(0.0, value);
    double scale = pow(10.0, (double)-digits);
    return round(value / scale) * scale;
}

/* ---- Random numbers ------------------------------------------------------ */

#define MN_DEFAULT_SEED 0x6772656d6c696eLL

static uint64_t mn_random[4];

/* Word 2 of the uniform data of every GPU block. */
static uint32_t mn_gpu_seed;

static uint64_t rotate_left(uint64_t value, int by) {
    return (value << by) | (value >> (64 - by));
}

/* xoshiro256** */
static uint64_t random_next(void) {
    uint64_t *s = mn_random;
    uint64_t result = rotate_left(s[1] * 5, 7) * 9;
    uint64_t t = s[1] << 17;
    s[2] ^= s[0];
    s[3] ^= s[1];
    s[1] ^= s[2];
    s[0] ^= s[3];
    s[2] ^= t;
    s[3] = rotate_left(s[3], 45);
    return result;
}

void mn_srand(int64_t seed) {
    mn_gpu_seed = (uint32_t)seed ^ (uint32_t)((uint64_t)seed >> 32);
    /* splitmix64 */
    uint64_t state = (uint64_t)seed;
    for (int i = 0; i < 4; i++) {
        uint64_t z = (state += 0x9e3779b97f4a7c15u);
        z = (z ^ (z >> 30)) * 0xbf58476d1ce4e5b9u;
        z = (z ^ (z >> 27)) * 0x94d049bb133111ebu;
        mn_random[i] = z ^ (z >> 31);
    }
}

void mn_srand_clock(void) {
    struct timespec now = {0, 0};
    clock_gettime(CLOCK_REALTIME, &now);
    mn_srand((int64_t)((uint64_t)now.tv_sec * 1000000000u +
                       (uint64_t)now.tv_nsec));
}

double mn_rand(void) {
    return (double)(random_next() >> 11) * (1.0 / 9007199254740992.0);
}

int64_t mn_rand_int(int64_t n, MnLoc loc) {
    if (n <= 0) {
        mn_panic(loc, "rand(%lld): the argument must be positive",
                 (long long)n);
    }
    /* Numbers below 2^64 mod n are passed over, which leaves a whole
     * number of runs of n. */
    uint64_t range = (uint64_t)n;
    uint64_t skipped = (0 - range) % range;
    for (;;) {
        uint64_t value = random_next();
        if (value >= skipped) return (int64_t)(value % range);
    }
}

/* ---- Time ---------------------------------------------------------------- */

static struct timespec mn_started;

double mn_clock(void) {
    struct timespec now = mn_started;
    clock_gettime(CLOCK_MONOTONIC, &now);
    return (double)(now.tv_sec - mn_started.tv_sec) +
           (double)(now.tv_nsec - mn_started.tv_nsec) * 1e-9;
}

void mn_sleep(double seconds) {
    if (!(seconds > 0.0)) return;
    fflush(stdout);
    struct timespec left;
    if (seconds >= 2147483647.0) {
        left.tv_sec = 2147483647;
        left.tv_nsec = 0;
    } else {
        double whole = floor(seconds);
        left.tv_sec = (time_t)whole;
        left.tv_nsec = (long)((seconds - whole) * 1e9);
        if (left.tv_nsec > 999999999) left.tv_nsec = 999999999;
    }
    while (nanosleep(&left, &left) == -1 && errno == EINTR) {
    }
}

/* ---- Several threads ----------------------------------------------------- */

/* A piece is this much work, in the units of a loop's weight. */
#define MN_PIECE_WORK UINT64_C(25000)
/* A loop with less work than this runs on the thread that reached it. */
#define MN_SPLIT_WORK UINT64_C(800000)
/* One thread takes part for every this much work. */
#define MN_THREAD_WORK UINT64_C(200000)
/* Where sums and products of weights stop. */
#define MN_WORK_MOST (UINT64_C(1) << 62)

#ifdef MN_THREADS
/* The words of MIDNIGHT_THREADS that are not the pool's. */
static struct {
    bool small;
    bool info;
} thread_options;
#endif

/* a * b for weights and row counts: the product, or MN_WORK_MOST when it
 * would be more. */
static uint64_t work_mul(uint64_t a, uint64_t b) {
    if (a != 0 && b > MN_WORK_MOST / a) return MN_WORK_MOST;
    uint64_t product = a * b;
    return product > MN_WORK_MOST ? MN_WORK_MOST : product;
}

/* a + b for weights, neither above MN_WORK_MOST. */
static uint64_t work_add(uint64_t a, uint64_t b) {
    uint64_t sum = a + b;
    return sum > MN_WORK_MOST ? MN_WORK_MOST : sum;
}

/* How the rows of a loop are divided. */
typedef struct MnSplit {
    /* Rows in a piece; the last piece may have fewer. */
    uint64_t piece_rows;
    uint64_t pieces;
    /* How many threads take part: 1 when the loop is not split. */
    uint32_t threads;
} MnSplit;

/* Divides a loop of `rows` rows whose runs weigh `weight` each. The size
 * of a piece and the choice to split depend on those two alone. */
static MnSplit split_of(uint64_t rows, uint64_t weight) {
    MnSplit split = {rows, 1, 1};
#ifdef MN_THREADS
    uint32_t count = mn_pool_threads();
    if (count < 2 || rows < 2) return split;
    if (weight == 0) weight = 1;
    uint64_t piece_rows = 1;
    uint64_t threads = rows;
    if (!thread_options.small) {
        uint64_t work = work_mul(rows, weight);
        if (work < MN_SPLIT_WORK) return split;
        piece_rows = MN_PIECE_WORK / weight;
        if (piece_rows == 0) piece_rows = 1;
        threads = work / MN_THREAD_WORK;
        if (threads < 2) threads = 2;
    }
    uint64_t pieces = (rows - 1) / piece_rows + 1;
    if (threads > pieces) threads = pieces;
    if (threads > count) threads = count;
    if (threads < 2) return split;
    split.piece_rows = piece_rows;
    split.pieces = pieces;
    split.threads = (uint32_t)threads;
#else
    (void)weight;
#endif
    return split;
}

#ifdef MN_THREADS
/* With `info`: the line for a loop that is split for the first time. */
static void split_report(MnLoc loc, uint64_t rows, const MnSplit *split) {
    if (!thread_options.info) return;
    fflush(stdout);
    fprintf(stderr, "midnight: threads: %s:%d: %llu row%s in %llu piece%s on %u threads\n",
            loc.file, (int)loc.line, (unsigned long long)rows,
            rows == 1 ? "" : "s", (unsigned long long)split->pieces,
            split->pieces == 1 ? "" : "s", (unsigned)split->threads);
}
#endif

/* The index of the table that holds row `row`, where `starts` has the
 * number of each table's first row, ascending, and then the number of
 * rows in all. */
static uint32_t table_of_row(const uint64_t *starts, uint32_t tables,
                             uint64_t row) {
    uint32_t low = 0;
    uint32_t high = tables;
    while (high - low > 1) {
        uint32_t middle = low + (high - low) / 2;
        if (starts[middle] <= row) {
            low = middle;
        } else {
            high = middle;
        }
    }
    return low;
}

#ifdef MN_THREADS
/* Reads MIDNIGHT_THREADS and sets the thread count. */
static void threads_start(void) {
    MnPoolOptions options = {0, false, false, false};
    static const struct {
        const char *word;
        int which;
    } words[] = {
        {"small", 0}, {"reverse", 1}, {"jitter", 2}, {"awake", 3}, {"info", 4},
    };
    const size_t word_count = sizeof words / sizeof words[0];
    thread_options.small = false;
    thread_options.info = false;
    const char *list = getenv("MIDNIGHT_THREADS");
    while (list && *list != '\0') {
        size_t len = strcspn(list, ",");
        size_t i = 0;
        while (i < word_count && (strlen(words[i].word) != len ||
                                  memcmp(words[i].word, list, len) != 0)) {
            i++;
        }
        uint32_t number = 0;
        bool digits = len != 0 && len <= 3;
        for (size_t at = 0; digits && at < len; at++) {
            digits = list[at] >= '0' && list[at] <= '9';
            number = number * 10 + (uint32_t)(list[at] - '0');
        }
        if (i < word_count) {
            switch (words[i].which) {
            case 0: thread_options.small = true; break;
            case 1: options.reverse = true; break;
            case 2: options.jitter = true; break;
            case 3: options.awake = true; break;
            default: thread_options.info = true; break;
            }
        } else if (digits && number >= 1 && number <= MN_POOL_MAX) {
            options.threads = number;
        } else if (len != 0) {
            fprintf(stderr, "midnight: MIDNIGHT_THREADS: unknown word `%.*s`\n",
                    (int)len, list);
        }
        list += len;
        if (*list == ',') list++;
    }
    mn_pool_start(&options);
    if (thread_options.info) {
        fprintf(stderr, "midnight: threads: %u\n", (unsigned)mn_pool_threads());
    }
}
#endif

/* ---- Independent loops ---------------------------------------------------- */

/* Makes the CPU copies of a loop's terms current, as the code of an
 * ordinary `each` does before its tables. */
static void loop_use(const MnLoop *loop) {
    if (loop->use_count == 0 || !mn_gpu_active) return;
    bool overwrites = false;
    for (uint32_t i = 0; i < loop->use_count; i++) {
        if (loop->uses[i].use == MN_USE_OVERWRITE) overwrites = true;
    }
    if (overwrites || mn_gpu_only) {
        mn_gpu_use(loop->query, loop->uses, loop->use_count);
    }
}

/* Reports the tables a loop assigned in: `flags` has wrote_count flags for
 * each of the tables, table after table. */
static void loop_wrote(const MnLoop *loop, MnTable *const *tables,
                       uint32_t table_count, const bool *flags) {
    for (uint32_t t = 0; t < table_count; t++) {
        for (uint32_t i = 0; i < loop->wrote_count; i++) {
            if (flags[(size_t)t * loop->wrote_count + i]) {
                mn_wrote(tables[t], loop->wrote[i]);
            }
        }
    }
}

/* Runs every row of a loop as one piece on the calling thread. Inside a
 * piece it leaves the world's state alone: the loop is part of that
 * piece. */
static void loop_whole(MnLoop *loop, const void *env, int64_t *sums) {
    MnQuery *query = loop->query;
    bool in_piece = MN_IN_PIECE;
    bool *wrote = NULL;
    if (!in_piece) {
        mn_world.defer_depth++;
        loop_use(loop);
        if (loop->wrote_count != 0 && mn_gpu_active) {
            wrote = mn_calloc(mn_mul(query->table_count, loop->wrote_count),
                              sizeof(bool));
        }
    }
    uint32_t last = query->table_count - 1;
    MnPiece piece = {query->tables, query->table_count, 0,
                     query->tables[last]->count, sums, wrote};
    loop->run(&piece, env);
    if (wrote) {
        loop_wrote(loop, query->tables, query->table_count, wrote);
        free(wrote);
    }
    mn_defer_leave();
}

#ifdef MN_THREADS
/* Bytes one thread collects while it runs pieces. */
typedef struct MnBytes {
    unsigned char *data;
    size_t len;
    size_t capacity;
} MnBytes;

/* What one piece queued, emitted of one event type, or drew: `count`
 * records in the bytes `begin` up to `end` of the buffer of that kind of the
 * thread that ran it. Kind 0 is queued changes; kind 1 + e is events of
 * type e; kind 1 + the number of events is draws. */
typedef struct MnStretch {
    uint64_t piece;
    uint32_t kind;
    uint32_t thread;
    size_t begin;
    size_t end;
    uint64_t count;
} MnStretch;

/* What a thread keeps for the pieces it runs of one loop. */
struct MnLoopThread {
    /* The piece it is running. */
    _Alignas(64) uint64_t piece;
    uint32_t index;
    /* Where a piece that fails goes on. */
    sigjmp_buf jump;
    /* One per kind. */
    MnBytes *buffers;
    /* Per kind: which of `stretches` is the newest of that kind, or
     * SIZE_MAX. */
    size_t *last;
    MnStretch *stretches;
    size_t stretch_count;
    size_t stretch_capacity;
    /* The totals of its pieces. */
    int64_t *sums;
    uint32_t sum_capacity;
    /* For each table with rows, the loop's wrote_count flags. */
    bool *wrote;
    size_t wrote_capacity;
};

/* What split loops share. One runs at a time. */
static struct {
    struct MnLoopThread *threads;
    uint32_t thread_count;
    /* The tables of the loop that have rows; `starts` has the number of
     * each one's first row, the rows being numbered through the tables in
     * order, and then the number of rows in all. */
    MnTable **tables;
    uint64_t *starts;
    uint32_t table_capacity;
    /* Working space for joining the pieces: every stretch; for each one,
     * the next stretch of its piece or UINT32_MAX; and for each piece, its
     * first stretch or UINT32_MAX. */
    MnStretch *joined;
    uint32_t *next;
    size_t joined_capacity;
    uint32_t *first;
    uint64_t first_capacity;
    /* The earliest piece that has failed, and its error. */
    pthread_mutex_t lock;
    uint64_t failed_piece;
    char *failed_text;
} loops = {.lock = PTHREAD_MUTEX_INITIALIZER};

static unsigned char *piece_bytes(struct MnLoopThread *thread, uint32_t kind,
                                  size_t need) {
    MnBytes *bytes = &thread->buffers[kind];
    if (bytes->capacity - bytes->len < need) {
        size_t capacity = bytes->capacity ? bytes->capacity : 1024;
        while (capacity - bytes->len < need) {
            if (capacity > SIZE_MAX / 2) mn_out_of_memory();
            capacity *= 2;
        }
        bytes->data = mn_realloc(bytes->data, capacity);
        bytes->capacity = capacity;
    }
    size_t newest = thread->last[kind];
    if (newest == SIZE_MAX || thread->stretches[newest].piece != thread->piece) {
        if (thread->stretch_count == thread->stretch_capacity) {
            size_t capacity =
                thread->stretch_capacity ? thread->stretch_capacity * 2 : 16;
            thread->stretches = mn_realloc(thread->stretches,
                                           mn_mul(capacity, sizeof(MnStretch)));
            thread->stretch_capacity = capacity;
        }
        newest = thread->stretch_count++;
        thread->stretches[newest] = (MnStretch){
            thread->piece, kind, thread->index, bytes->len, bytes->len, 0};
        thread->last[kind] = newest;
    }
    unsigned char *at = bytes->data + bytes->len;
    bytes->len += need;
    thread->stretches[newest].end = bytes->len;
    thread->stretches[newest].count++;
    return at;
}

static _Noreturn void piece_fail(struct MnLoopThread *thread, char *text) {
    pthread_mutex_lock(&loops.lock);
    if (thread->piece < loops.failed_piece) {
        free(loops.failed_text);
        loops.failed_text = text;
        loops.failed_piece = thread->piece;
    } else {
        free(text);
    }
    pthread_mutex_unlock(&loops.lock);
    mn_pool_fail(thread->piece);
    siglongjmp(thread->jump, 1);
}

/* Every piece before the earliest failed one has finished: reports its
 * error, which is the one the loop stops with on one thread. */
static void loop_failed(void *context, uint64_t piece) {
    (void)context;
    (void)piece;
    pthread_mutex_lock(&loops.lock);
    fflush(stdout);
    fputs(loops.failed_text, stderr);
    mn_leave(1);
}

/* One run of a split loop. */
typedef struct LoopRun {
    const MnLoop *loop;
    const void *env;
    uint64_t rows;
    uint64_t piece_rows;
    /* How many tables have rows. */
    uint32_t table_count;
    /* The pieces record the tables they assign in. */
    bool flags;
} LoopRun;

static void loop_piece(void *context, uint64_t piece, uint32_t index) {
    const LoopRun *run = context;
    const MnLoop *loop = run->loop;
    struct MnLoopThread *thread = &loops.threads[index];
    uint64_t from = piece * run->piece_rows;
    uint64_t to = run->rows - from < run->piece_rows ? run->rows
                                                      : from + run->piece_rows;
    uint32_t first = table_of_row(loops.starts, run->table_count, from);
    uint32_t last = table_of_row(loops.starts, run->table_count, to - 1);
    MnPiece rows = {
        loops.tables + first,
        last - first + 1,
        (uint32_t)(from - loops.starts[first]),
        (uint32_t)(to - loops.starts[last]),
        thread->sums,
        run->flags ? thread->wrote + (size_t)first * loop->wrote_count : NULL,
    };
    thread->piece = piece;
    mn_loop_thread = thread;
    if (sigsetjmp(thread->jump, 0) == 0) loop->run(&rows, run->env);
    mn_loop_thread = NULL;
}

static int stretch_order(const void *a, const void *b) {
    const MnStretch *x = a;
    const MnStretch *y = b;
    if (x->piece != y->piece) return x->piece < y->piece ? -1 : 1;
    return x->kind < y->kind ? -1 : x->kind > y->kind;
}

/* Hands one stretch to the world: queued changes to the end of the queue,
 * events to the end of their buffer, draws to the end of the list. */
static void stretch_append(const MnStretch *stretch) {
    const MnBytes *bytes = &loops.threads[stretch->thread].buffers[stretch->kind];
    size_t size = stretch->end - stretch->begin;
    if (stretch->kind == 0) {
        memcpy(commands_extend(size), bytes->data + stretch->begin, size);
        mn_world.deferred_count += (uint32_t)stretch->count;
    } else if (stretch->kind == 1 + mn_world.program->event_count) {
#ifdef MN_WINDOW
        memcpy(draws_extend(&drawing.list, (size_t)stretch->count),
               bytes->data + stretch->begin, size);
#endif
    } else {
        memcpy(events_extend(stretch->kind - 1, stretch->count),
               bytes->data + stretch->begin, size);
    }
}

/* Appends what the pieces queued to the world's queue and what they
 * emitted to the event buffers, piece after piece: the order in which one
 * thread queues and emits. A piece has one stretch of a kind at most, and
 * the kinds go to different places, so only the order of the pieces
 * counts. */
static void loop_join(uint32_t threads, uint64_t pieces) {
    size_t total = 0;
    for (uint32_t t = 0; t < threads; t++) {
        total += loops.threads[t].stretch_count;
    }
    if (total == 0) return;
    if (total > loops.joined_capacity) {
        loops.joined = mn_realloc(loops.joined, mn_mul(total, sizeof(MnStretch)));
        loops.next = mn_realloc(loops.next, mn_mul(total, sizeof(uint32_t)));
        loops.joined_capacity = total;
    }
    size_t at = 0;
    for (uint32_t t = 0; t < threads; t++) {
        const struct MnLoopThread *thread = &loops.threads[t];
        if (thread->stretch_count == 0) continue;
        memcpy(loops.joined + at, thread->stretches,
               thread->stretch_count * sizeof(MnStretch));
        at += thread->stretch_count;
    }
    /* With stretches for few of very many pieces, or too many to number
     * in 32 bits, they are sorted. */
    if (pieces / 64 > total || total >= UINT32_MAX) {
        qsort(loops.joined, total, sizeof(MnStretch), stretch_order);
        for (size_t i = 0; i < total; i++) stretch_append(&loops.joined[i]);
        return;
    }
    /* Otherwise each piece gets the list of its stretches. */
    if (pieces > loops.first_capacity) {
        loops.first = mn_realloc(loops.first, mn_mul(pieces, sizeof(uint32_t)));
        loops.first_capacity = pieces;
    }
    memset(loops.first, 0xff, (size_t)pieces * sizeof(uint32_t));
    for (size_t i = 0; i < total; i++) {
        uint64_t piece = loops.joined[i].piece;
        loops.next[i] = loops.first[piece];
        loops.first[piece] = (uint32_t)i;
    }
    for (uint64_t piece = 0; piece < pieces; piece++) {
        for (uint32_t i = loops.first[piece]; i != UINT32_MAX; i = loops.next[i]) {
            stretch_append(&loops.joined[i]);
        }
    }
}

/* Runs the rows of a loop in pieces on several threads. */
static void loop_split(MnLoop *loop, const void *env, int64_t *sums,
                       uint64_t rows, const MnSplit *split) {
    MnWorld *w = &mn_world;
    MnQuery *query = loop->query;
    w->defer_depth++;
    loop_use(loop);

    if (query->table_count > loops.table_capacity) {
        uint32_t capacity = query->table_capacity;
        loops.tables =
            mn_realloc(loops.tables, mn_mul(capacity, sizeof(MnTable *)));
        loops.starts = mn_realloc(loops.starts,
                                  mn_mul((size_t)capacity + 1, sizeof(uint64_t)));
        loops.table_capacity = capacity;
    }
    uint32_t tables = 0;
    uint64_t numbered = 0;
    for (uint32_t t = 0; t < query->table_count; t++) {
        MnTable *table = query->tables[t];
        if (table->count == 0) continue;
        loops.tables[tables] = table;
        loops.starts[tables++] = numbered;
        numbered += table->count;
    }
    loops.starts[tables] = numbered;

    if (!loops.threads) {
        loops.thread_count = mn_pool_threads();
        size_t size = mn_mul(loops.thread_count, sizeof(struct MnLoopThread));
        loops.threads = aligned_alloc(_Alignof(struct MnLoopThread), size);
        if (!loops.threads) mn_out_of_memory();
        memset(loops.threads, 0, size);
    }
    uint32_t kinds = 2 + w->program->event_count;
    bool flags = loop->wrote_count != 0 && mn_gpu_active;
    size_t flag_count = flags ? mn_mul(tables, loop->wrote_count) : 0;
    for (uint32_t t = 0; t < split->threads; t++) {
        struct MnLoopThread *thread = &loops.threads[t];
        thread->index = t;
        if (!thread->buffers) {
            thread->buffers = mn_calloc(kinds, sizeof(MnBytes));
            thread->last = mn_calloc(kinds, sizeof(size_t));
        }
        for (uint32_t kind = 0; kind < kinds; kind++) {
            thread->buffers[kind].len = 0;
            thread->last[kind] = SIZE_MAX;
        }
        thread->stretch_count = 0;
        if (loop->sum_count > thread->sum_capacity) {
            thread->sums = mn_realloc(
                thread->sums, mn_mul(loop->sum_count, sizeof(int64_t)));
            thread->sum_capacity = loop->sum_count;
        }
        for (uint32_t i = 0; i < loop->sum_count; i++) thread->sums[i] = 0;
        if (flag_count > thread->wrote_capacity) {
            thread->wrote = mn_realloc(thread->wrote, flag_count);
            thread->wrote_capacity = flag_count;
        }
        for (size_t i = 0; i < flag_count; i++) thread->wrote[i] = false;
    }
    loops.failed_piece = UINT64_MAX;
    if (!loop->reported) {
        loop->reported = true;
        split_report(loop->loc, rows, split);
    }

    LoopRun run = {loop, env, rows, split->piece_rows, tables, flags};
    MnPoolLoop pieces = {loop_piece, loop_failed, &run, split->pieces,
                         split->threads};
    mn_pool_loop(&pieces);

    loop_join(split->threads, split->pieces);
    for (uint32_t t = 0; t < split->threads; t++) {
        const struct MnLoopThread *thread = &loops.threads[t];
        for (uint32_t i = 0; i < loop->sum_count; i++) {
            sums[i] = (int64_t)((uint64_t)sums[i] + (uint64_t)thread->sums[i]);
        }
    }
    if (flags) {
        /* The flags of thread 0 become those of every thread. */
        bool *all = loops.threads[0].wrote;
        for (uint32_t t = 1; t < split->threads; t++) {
            for (size_t i = 0; i < flag_count; i++) {
                all[i] |= loops.threads[t].wrote[i];
            }
        }
        loop_wrote(loop, loops.tables, tables, all);
    }
    if (--w->defer_depth == 0 && w->deferred_count != 0) mn_flush();
}

/* Releases what split loops kept. */
static void loops_finish(void) {
    for (uint32_t t = 0; t < loops.thread_count; t++) {
        struct MnLoopThread *thread = &loops.threads[t];
        if (thread->buffers) {
            uint32_t kinds = 2 + mn_world.program->event_count;
            for (uint32_t kind = 0; kind < kinds; kind++) {
                free(thread->buffers[kind].data);
            }
        }
        free(thread->buffers);
        free(thread->last);
        free(thread->stretches);
        free(thread->sums);
        free(thread->wrote);
    }
    free(loops.threads);
    free(loops.tables);
    free(loops.starts);
    free(loops.joined);
    free(loops.next);
    free(loops.first);
    free(loops.failed_text);
    loops.threads = NULL;
    loops.thread_count = 0;
    loops.tables = NULL;
    loops.starts = NULL;
    loops.table_capacity = 0;
    loops.joined = NULL;
    loops.next = NULL;
    loops.joined_capacity = 0;
    loops.first = NULL;
    loops.first_capacity = 0;
    loops.failed_text = NULL;
}
#endif

void mn_loop(MnLoop *loop, const void *env, int64_t *sums) {
    for (uint32_t i = 0; i < loop->sum_count; i++) sums[i] = 0;
    MnQuery *query = loop->query;
    bool in_piece = MN_IN_PIECE;
    /* Inside a piece the query is up to date: the loop that is split
     * lists it. */
    if (!in_piece) mn_query_update(query);
    if (query->table_count == 0) return;
#ifdef MN_THREADS
    if (!in_piece && mn_world.defer_depth == 0 && mn_pool_threads() > 1) {
        for (uint32_t i = 0; i < loop->query_count; i++) {
            mn_query_update(loop->queries[i]);
        }
        uint64_t rows = mn_query_rows(query);
        MnSplit split = split_of(rows, loop->weight());
        if (split.threads > 1) {
            loop_split(loop, env, sums, rows, &split);
            return;
        }
    }
#endif
    loop_whole(loop, env, sums);
}

/* ---- GPU systems --------------------------------------------------------- */

bool mn_gpu_active = false;
bool mn_gpu_only = false;
bool mn_gpu_rows = false;

uint64_t mn_gpu_uploads;
uint64_t mn_gpu_downloads;
uint64_t mn_gpu_rows_up;
uint64_t mn_gpu_rows_down;
uint64_t mn_gpu_submissions;
uint64_t mn_gpu_waits;

#ifndef MN_GPU
/* No column has a GPU copy. */
void mn_gpu_row(MnTable *table, uint32_t row, uint32_t component,
                uint32_t use) {
    (void)table;
    (void)row;
    (void)component;
    (void)use;
}

void mn_gpu_use(MnQuery *query, const MnUse *uses, uint32_t count) {
    (void)query;
    (void)uses;
    (void)count;
}

void mn_gpu_wrote(MnTable *table, uint32_t component) {
    (void)table;
    (void)component;
}
#endif

const MnGpuCheck *mn_gpu_check;

/* A tick has ended whose `stats` line is not printed yet. */
static bool stats_pending;

/* What mn_gpu_run keeps for a block between calls. */
typedef struct MnGpuScratch {
    MnGpuBlock *block;
    /* The block that ran for the first time before this one. */
    struct MnGpuScratch *next;
    uint32_t *uniform;
    /* For a run on the CPU: the outer query's tables that have rows.
     * `starts` has the number of each one's first row, the rows being
     * numbered through the tables in order, and then the number of rows
     * in all; `outer` has each one's outer columns, outer_count for each
     * table. */
    uint32_t table_count;
    uint32_t table_capacity;
    uint64_t *starts;
    void **outer;
    const void **inner;
    /* One buffer per inner column, grown as needed. */
    void **snapshots;
    size_t *snapshot_sizes;
    /* What midnight_gpu.c keeps for the block. */
    void *device;
    /* The `info` line that says why the block runs on the CPU has been
     * printed. */
    bool reported;
    /* The `info` line that says how the block is split has been
     * printed. */
    bool split_reported;
    /* Where a run on the CPU writes its records while no window is open. */
    MnDraw *records;
    size_t record_capacity;
} MnGpuScratch;

/* The block that ran for the first time most recently. */
static MnGpuScratch *gpu_blocks;

static MnGpuScratch *gpu_scratch(MnGpuBlock *block) {
    if (block->state) return block->state;
    MnGpuScratch *scratch = mn_calloc(1, sizeof *scratch);
    scratch->block = block;
    scratch->next = gpu_blocks;
    gpu_blocks = scratch;
    scratch->uniform = mn_calloc(block->uniform_words, sizeof(uint32_t));
    scratch->inner = mn_calloc(block->inner_count + 1, sizeof(void *));
    scratch->snapshots = mn_calloc(block->inner_count + 1, sizeof(void *));
    scratch->snapshot_sizes = mn_calloc(block->inner_count + 1, sizeof(size_t));
    block->state = scratch;
    return scratch;
}

static void *gpu_column(const MnTable *table, const MnGpuColumn *column) {
    if (column->component == MN_GPU_ENTITIES) return table->entities;
    return table->columns[column->component];
}

static bool gpu_writes(const MnGpuBlock *block, uint32_t component) {
    for (uint32_t i = 0; i < block->outer_count; i++) {
        const MnGpuColumn *column = &block->outer_columns[i];
        if (column->component == component && column->write) return true;
    }
    return false;
}

/* Points scratch->inner at the snapshot of each inner column and returns
 * its row count. A column the block does not write, in the only table
 * with rows, stands for itself; anything else is copied. */
static uint32_t gpu_snapshot(const MnGpuBlock *block, MnGpuScratch *scratch) {
    MnQuery *query = block->inner;
    if (!query) return 0;
    uint64_t rows = 0;
    uint32_t populated = 0;
    const MnTable *only = NULL;
    for (uint32_t t = 0; t < query->table_count; t++) {
        const MnTable *table = query->tables[t];
        if (table->count == 0) continue;
        rows += table->count;
        populated++;
        only = table;
    }
    if (rows > UINT32_MAX) mn_out_of_memory();
    for (uint32_t i = 0; i < block->inner_count; i++) {
        const MnGpuColumn *column = &block->inner_columns[i];
        if (populated == 1 && !gpu_writes(block, column->component)) {
            scratch->inner[i] = gpu_column(only, column);
            continue;
        }
        size_t size = mn_mul((size_t)rows, column->size);
        if (size > scratch->snapshot_sizes[i]) {
            scratch->snapshots[i] = mn_realloc(scratch->snapshots[i], size);
            scratch->snapshot_sizes[i] = size;
        }
        char *out = scratch->snapshots[i];
        for (uint32_t t = 0; t < query->table_count; t++) {
            const MnTable *table = query->tables[t];
            if (table->count == 0) continue;
            size_t bytes = (size_t)table->count * column->size;
            memcpy(out, gpu_column(table, column), bytes);
            out += bytes;
        }
        scratch->inner[i] = scratch->snapshots[i];
    }
    return (uint32_t)rows;
}

/* Lists the tables of the outer query that have rows, with their outer
 * columns, and returns the number of rows in all. */
static uint64_t gpu_tables(const MnGpuBlock *block, MnGpuScratch *scratch) {
    const MnQuery *query = block->outer;
    if (query->table_count > scratch->table_capacity) {
        uint32_t capacity = query->table_capacity;
        scratch->starts =
            mn_realloc(scratch->starts, mn_mul((size_t)capacity + 1, sizeof(uint64_t)));
        scratch->outer = mn_realloc(
            scratch->outer,
            mn_mul(mn_mul(capacity, block->outer_count), sizeof(void *)));
        scratch->table_capacity = capacity;
    }
    uint32_t count = 0;
    uint64_t rows = 0;
    for (uint32_t t = 0; t < query->table_count; t++) {
        const MnTable *table = query->tables[t];
        if (table->count == 0) continue;
        void **outer = scratch->outer + (size_t)count * block->outer_count;
        for (uint32_t i = 0; i < block->outer_count; i++) {
            outer[i] = gpu_column(table, &block->outer_columns[i]);
        }
        scratch->starts[count++] = rows;
        rows += table->count;
    }
    if (count != 0) scratch->starts[count] = rows;
    scratch->table_count = count;
    return rows;
}

/* The lowest failure site one thread has recorded. */
typedef struct GpuError {
    _Alignas(64) uint32_t site;
} GpuError;

/* A run of a block on the CPU. */
typedef struct GpuLoop {
    const MnGpuBlock *block;
    const MnGpuScratch *scratch;
    uint64_t rows;
    uint64_t piece_rows;
    /* One for each thread taking part, when the run is split. */
    GpuError *errors;
    /* When the block draws: the records of all its rows, row after row. */
    MnDraw *records;
} GpuLoop;

/* The records of a run on the CPU of a block that draws, `rows` rows of
 * them: at the end of the frame's list while a window is open, otherwise
 * memory of the block's that nothing reads. */
static MnDraw *gpu_records(const MnGpuBlock *block, MnGpuScratch *scratch,
                           uint64_t rows) {
    size_t count = mn_mul((size_t)rows, block->draws);
#ifdef MN_WINDOW
    if (win.open) return draws_extend(&drawing.list, count);
#endif
    if (count > scratch->record_capacity) {
        scratch->records =
            mn_realloc(scratch->records, mn_mul(count, sizeof(MnDraw)));
        scratch->record_capacity = count;
    }
    return scratch->records;
}

/* Runs the rows numbered `from` up to `to`, table after table. */
static void gpu_rows(const GpuLoop *loop, uint64_t from, uint64_t to,
                     uint32_t *error) {
    const MnGpuScratch *scratch = loop->scratch;
    const MnGpuBlock *block = loop->block;
    uint32_t t = table_of_row(scratch->starts, scratch->table_count, from);
    while (from < to) {
        uint64_t start = scratch->starts[t];
        uint64_t stop = scratch->starts[t + 1] < to ? scratch->starts[t + 1] : to;
        MnGpuRun run = {scratch->uniform,
                        scratch->outer + (size_t)t * block->outer_count,
                        scratch->inner,
                        error,
                        (uint32_t)(from - start),
                        (uint32_t)(stop - start),
                        loop->records ? loop->records + (size_t)start * block->draws
                                      : NULL};
        block->cpu(&run);
        from = stop;
        t++;
    }
}

#ifdef MN_THREADS
static void gpu_piece(void *context, uint64_t piece, uint32_t thread) {
    GpuLoop *loop = context;
    uint64_t from = piece * loop->piece_rows;
    uint64_t to = loop->rows - from < loop->piece_rows ? loop->rows
                                                        : from + loop->piece_rows;
    uint32_t error = UINT32_MAX;
    gpu_rows(loop, from, to, &error);
    if (error < loop->errors[thread].site) loop->errors[thread].site = error;
}
#endif

void mn_gpu_run(MnGpuBlock *block) {
    MnGpuScratch *scratch = gpu_scratch(block);
    MnQuery *query = block->outer;
    mn_query_update(query);
    if (block->inner) mn_query_update(block->inner);

    uint32_t *uniform = scratch->uniform;
    float dt = (float)mn_time.dt;
    float elapsed = (float)mn_time.elapsed;
    uniform[2] = mn_gpu_seed;
    uniform[3] = block->id;
    uniform[4] = (uint32_t)mn_time.tick;
    memcpy(&uniform[5], &dt, sizeof dt);
    memcpy(&uniform[6], &elapsed, sizeof elapsed);
    uniform[7] = 0;
    if (block->fill) block->fill(uniform);

    const char *reason = "no device";
#ifdef MN_GPU
    reason = mn_gpu_device_run(block, uniform, &scratch->device);
    if (!reason) return;
#endif
    if (gpu_options.info && !scratch->reported) {
        scratch->reported = true;
        fprintf(stderr, "midnight: gpu: %s (each %u) runs on the CPU: %s\n",
                block->system, (unsigned)block->each, reason);
    }

    uniform[1] = gpu_snapshot(block, scratch);
    uint64_t rows = gpu_tables(block, scratch);
    if (rows == 0) return;
    uint64_t weight =
        work_add(block->weight, work_mul(block->inner_weight, uniform[1]));
    MnSplit split = split_of(rows, weight);
    GpuLoop loop = {block, scratch, rows, split.piece_rows, NULL, NULL};
    if (block->draws != 0) loop.records = gpu_records(block, scratch, rows);
    uint32_t error = UINT32_MAX;
    if (split.threads < 2) {
        gpu_rows(&loop, 0, rows, &error);
    } else {
#ifdef MN_THREADS
        if (!scratch->split_reported) {
            scratch->split_reported = true;
            split_report(block->loc, rows, &split);
        }
        GpuError errors[MN_POOL_MAX];
        for (uint32_t t = 0; t < split.threads; t++) {
            errors[t].site = UINT32_MAX;
        }
        loop.errors = errors;
        MnPoolLoop pieces = {gpu_piece, NULL, &loop, split.pieces,
                             split.threads};
        mn_pool_loop(&pieces);
        for (uint32_t t = 0; t < split.threads; t++) {
            if (errors[t].site < error) error = errors[t].site;
        }
#endif
    }
    if (error != UINT32_MAX) {
        const MnGpuSite *site = &block->sites[error];
        mn_panic(site->loc, "%s", site->message);
    }
}

void mn_gpu_submit(void) {
#ifdef MN_GPU
    mn_gpu_device_submit();
#endif
}

#ifdef MN_GPU
const char *mn_gpu_strict_refused;
#endif

#if defined(MN_GPU) && defined(MN_GPU_SYSTEMS)
/* Runs the strict check on `device` and on the CPU, sets
 * mn_gpu_strict_refused when they differ or with `strict-cpu`, and prints
 * the `info` line that says where strict blocks run. */
static void gpu_strict_check(const char *device) {
    static char reason[400];
    const MnGpuCheck *check = mn_gpu_check;
    if (gpu_options.strict_cpu) {
        snprintf(reason, sizeof reason, "MIDNIGHT_GPU has strict-cpu");
    } else {
        size_t words = (size_t)check->rows * check->results;
        uint32_t *on_device = mn_calloc(words, sizeof(uint32_t));
        uint32_t *on_cpu = mn_calloc(words, sizeof(uint32_t));
        const char *unable = mn_gpu_device_check(check, on_device);
        check->cpu(check->inputs, on_cpu);
        if (unable) {
            snprintf(reason, sizeof reason, "%s can't run the strict check: %s",
                     device, unable);
        } else {
            for (size_t i = 0; i < words; i++) {
                if (on_device[i] == on_cpu[i]) continue;
                snprintf(reason, sizeof reason,
                         "%s failed the strict check (result %u of row %u is "
                         "0x%08x on the device and 0x%08x on the CPU)",
                         device, (unsigned)(i % check->results),
                         (unsigned)(i / check->results), (unsigned)on_device[i],
                         (unsigned)on_cpu[i]);
                break;
            }
        }
        free(on_device);
        free(on_cpu);
    }
    if (reason[0] != '\0') mn_gpu_strict_refused = reason;
    if (!gpu_options.info) return;
    if (mn_gpu_strict_refused) {
        fprintf(stderr, "midnight: gpu: strict blocks run on the CPU: %s\n",
                reason);
    } else {
        fprintf(stderr, "midnight: gpu: strict check passed\n");
    }
}
#endif

#if defined(MN_GPU_SYSTEMS) || defined(MN_WINDOW)
/* Reads MIDNIGHT_GPU, once. */
static void gpu_read_options(void) {
    static bool read;
    if (read) return;
    read = true;
    static const struct {
        const char *word;
        bool *option;
    } words[] = {
        {"off", &gpu_options.off},
        {"software", &gpu_options.software},
        {"info", &gpu_options.info},
        {"stats", &gpu_options.stats},
        {"rows", &gpu_options.rows},
        {"strict-cpu", &gpu_options.strict_cpu},
    };
    const size_t word_count = sizeof words / sizeof words[0];
    const char *list = getenv("MIDNIGHT_GPU");
    while (list && *list != '\0') {
        size_t len = strcspn(list, ",");
        size_t i = 0;
        while (i < word_count && (strlen(words[i].word) != len ||
                                  memcmp(words[i].word, list, len) != 0)) {
            i++;
        }
        if (i < word_count) {
            *words[i].option = true;
        } else if (len != 0) {
            fprintf(stderr, "midnight: MIDNIGHT_GPU: unknown word `%.*s`\n",
                    (int)len, list);
        }
        list += len;
        if (*list == ',') list++;
    }
}
#endif

#ifdef MN_GPU
/* The device has been asked for. */
static bool gpu_started;

/* Starts the device, fitting `surface` when that is not NULL, unless it
 * has been asked for before or MIDNIGHT_GPU has `off`, and prints the
 * `info` line. Returns its name, or NULL when there is none. */
static const char *gpu_device_start(struct WGPUSurfaceImpl *surface) {
    if (gpu_started) return mn_gpu_device_name();
    gpu_started = true;
    const char *device = NULL;
    if (!gpu_options.off) {
        device = mn_gpu_device_start(gpu_options.software, surface);
    }
    if (gpu_options.info) {
        fflush(stdout);
        fprintf(stderr, "midnight: gpu: %s\n", device ? device : "no device");
    }
    return device;
}
#endif

#ifdef MN_GPU_SYSTEMS
/* Reads MIDNIGHT_GPU, chooses the device and prints the `info` line. */
static void gpu_start(void) {
    gpu_read_options();
    mn_gpu_rows = gpu_options.rows;
#ifdef MN_GPU
    const char *device = gpu_device_start(NULL);
    if (device && mn_gpu_check) gpu_strict_check(device);
#else
    if (gpu_options.info) fprintf(stderr, "midnight: gpu: no device\n");
#endif
}

/* Waits for the device. With `stats`, the tick's line is printed when the
 * next tick starts or the program ends, so that it also counts what the
 * top-level code after the tick transfers. */
static void gpu_tick_end(void) {
#ifdef MN_GPU
    mn_gpu_device_wait();
#endif
    if (gpu_options.stats) stats_pending = true;
}
#endif

/* Prints the `stats` line of the last tick that ended, if it has not been
 * printed. */
static void gpu_stats_line(void) {
    if (!stats_pending) return;
    stats_pending = false;
    fflush(stdout);
    fprintf(stderr,
            "midnight: gpu: tick %lld: %llu upload%s, %llu download%s, "
            "%llu row%s up, %llu row%s down, %llu submission%s, %llu wait%s\n",
            (long long)(mn_time.tick - 1), (unsigned long long)mn_gpu_uploads,
            mn_gpu_uploads == 1 ? "" : "s",
            (unsigned long long)mn_gpu_downloads,
            mn_gpu_downloads == 1 ? "" : "s",
            (unsigned long long)mn_gpu_rows_up, mn_gpu_rows_up == 1 ? "" : "s",
            (unsigned long long)mn_gpu_rows_down,
            mn_gpu_rows_down == 1 ? "" : "s",
            (unsigned long long)mn_gpu_submissions,
            mn_gpu_submissions == 1 ? "" : "s",
            (unsigned long long)mn_gpu_waits, mn_gpu_waits == 1 ? "" : "s");
    mn_gpu_uploads = 0;
    mn_gpu_downloads = 0;
    mn_gpu_rows_up = 0;
    mn_gpu_rows_down = 0;
    mn_gpu_submissions = 0;
    mn_gpu_waits = 0;
}

/* Releases what mn_gpu_run kept for every block, then the device. */
static void gpu_finish(void) {
    while (gpu_blocks) {
        MnGpuScratch *scratch = gpu_blocks;
        gpu_blocks = scratch->next;
#ifdef MN_GPU
        mn_gpu_device_forget(scratch->device);
#endif
        for (uint32_t i = 0; i < scratch->block->inner_count; i++) {
            free(scratch->snapshots[i]);
        }
        free(scratch->uniform);
        free(scratch->starts);
        free(scratch->outer);
        free(scratch->inner);
        free(scratch->snapshots);
        free(scratch->snapshot_sizes);
        free(scratch->records);
        scratch->block->state = NULL;
        free(scratch);
    }
#ifdef MN_GPU
    mn_gpu_device_finish();
#endif
}

/* ---- Start and finish ---------------------------------------------------- */

void mn_start(const MnProgram *program) {
    MnWorld *w = &mn_world;
    memset(w, 0, sizeof *w);
    w->program = program;
    w->free_head = MN_NONE;
    size_t component_count = program->component_count;
    w->scratch_set = mn_malloc((component_count + 1) * sizeof(uint32_t));
    w->scratch_values = mn_malloc((component_count + 1) * sizeof(void *));
    w->events = mn_calloc(program->event_count, sizeof(MnEventBuf));
    w->table_map_capacity = 64;
    w->table_map = mn_calloc(w->table_map_capacity, sizeof(uint32_t));
    table_for_set(w->scratch_set, 0);
    mn_srand(MN_DEFAULT_SEED);
#ifdef MN_THREADS
    threads_start();
#endif
#ifdef MN_GPU_SYSTEMS
    gpu_start();
#endif
#ifdef MN_WINDOW
    window_start();
#endif
    clock_gettime(CLOCK_MONOTONIC, &mn_started);
}

#ifdef MN_DEBUG
/* Drops every value the world holds and frees the world's memory. */
static void world_destroy(void) {
    MnWorld *w = &mn_world;
    const MnProgram *program = w->program;

    for (uint32_t t = 0; t < w->table_count; t++) {
        MnTable *table = w->tables[t];
        if (table->drop_count != 0) {
            for (uint32_t row = 0; row < table->count; row++) {
                table_drop_row(table, row);
            }
        }
    }
    for (uint32_t i = 0; i < program->event_count; i++) {
        events_discard(i, w->events[i].end);
    }
    commands_run(false);
    if (program->drop_globals) program->drop_globals();

    for (uint32_t t = 0; t < w->table_count; t++) {
        MnTable *table = w->tables[t];
        if (table->capacity != 0) {
            for (uint32_t i = 0; i < table->data_count; i++) {
                free(table->columns[table->data[i]]);
            }
        }
        free(table->entities);
        free(table);
    }
    for (uint32_t i = 0; i < program->event_count; i++) {
        free(w->events[i].data);
    }
    free(w->tables);
    free(w->slots);
    free(w->table_map);
    free(w->commands);
    free(w->events);
    free(w->scratch_set);
    free(w->scratch_values);
    memset(w, 0, sizeof *w);
    w->program = program;
    w->free_head = MN_NONE;
}
#endif

void mn_finish(void) {
#ifdef MN_WINDOW
    window_finish();
#endif
    gpu_stats_line();
    fflush(stdout);
    gpu_finish();
#ifdef MN_THREADS
    mn_pool_finish();
    loops_finish();
#endif
#ifdef MN_DEBUG
    world_destroy();
    if (mn_live_buffers != 0) {
        fprintf(stderr, "midnight: %lld buffer%s still alive at exit\n",
                (long long)mn_live_buffers, mn_live_buffers == 1 ? "" : "s");
        mn_leave(70);
    }
#endif
}
