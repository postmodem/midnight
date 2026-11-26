/*
 * The Midnight runtime. See midnight_rt.h for what each function promises.
 */

#define _POSIX_C_SOURCE 200809L

#include "midnight_rt.h"

/* MN_GPU_SYSTEMS is defined when the program declares a GPU system, and
 * MN_GPU as well when midnight_gpu.c is compiled with it. */
#ifdef MN_GPU
#include "midnight_gpu.h"
#ifndef MN_GPU_SYSTEMS
#define MN_GPU_SYSTEMS
#endif
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

_Noreturn void mn_panic(MnLoc loc, const char *format, ...) {
    fflush(stdout);
    fprintf(stderr, "%s:%d: runtime error: ", loc.file, (int)loc.line);
    va_list args;
    va_start(args, format);
    vfprintf(stderr, format, args);
    va_end(args);
    fputc('\n', stderr);
    mn_leave(1);
}

static void gpu_stats_line(void);

_Noreturn void mn_exit(int64_t status) {
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

/* Appends a record with `payload` bytes after its start and returns the
 * address of the payload. */
static unsigned char *command_push(uint32_t kind, uint32_t count, MnEntity e,
                                   size_t payload) {
    MnWorld *w = &mn_world;
    size_t need = sizeof(MnCommand) + payload;
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
    MnCommand command = {kind, count, e};
    memcpy(at, &command, sizeof command);
    w->command_len += need;
    w->deferred_count++;
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

void *mn_emit(uint32_t event) {
    MnEventBuf *buf = &mn_world.events[event];
    size_t size = mn_world.program->events[event].size;
    uint64_t count = buf->end - buf->base;
    if (count == buf->capacity) {
        if (buf->capacity > SIZE_MAX / 2) mn_out_of_memory();
        uint64_t capacity = buf->capacity ? buf->capacity * 2 : 16;
        buf->data = mn_realloc(buf->data, mn_mul((size_t)capacity, size));
        buf->capacity = capacity;
    }
    void *slot = buf->data + count * size;
    memset(slot, 0, size);
    buf->end++;
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

/* ---- Systems and ticks --------------------------------------------------- */

#ifdef MN_GPU_SYSTEMS
static void gpu_tick_end(void);
#endif

static void tick(void) {
    MnWorld *w = &mn_world;
    const MnProgram *program = w->program;
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
 * reports whether that text reads back as the same number. */
static bool float_reads_back(double value, int precision, char *text,
                             size_t size) {
    snprintf(text, size, "%.*e", precision - 1, value);
    return strtod(text, NULL) == value;
}

static int format_float(double value, char *out) {
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
     * no smaller number does. */
    char text[32];
    int precision = float_reads_back(value, 15, text, sizeof text) ? 1 : 16;
    while (precision < 17 &&
           !float_reads_back(value, precision, text, sizeof text)) {
        precision++;
    }
    if (precision == 17) snprintf(text, sizeof text, "%.16e", value);

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

/* The words of MIDNIGHT_GPU. */
static struct {
    bool off;
    bool software;
    bool info;
    bool stats;
    bool rows;
    bool strict_cpu;
} gpu_options;

/* A tick has ended whose `stats` line is not printed yet. */
static bool stats_pending;

/* What mn_gpu_run keeps for a block between calls. */
typedef struct MnGpuScratch {
    MnGpuBlock *block;
    /* The block that ran for the first time before this one. */
    struct MnGpuScratch *next;
    uint32_t *uniform;
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
    scratch->outer = mn_calloc(block->outer_count + 1, sizeof(void *));
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
    uint32_t error = UINT32_MAX;
    MnGpuRun run = {uniform, scratch->outer, scratch->inner, &error};
    for (uint32_t t = 0; t < query->table_count; t++) {
        const MnTable *table = query->tables[t];
        if (table->count == 0) continue;
        uniform[0] = table->count;
        for (uint32_t i = 0; i < block->outer_count; i++) {
            scratch->outer[i] = gpu_column(table, &block->outer_columns[i]);
        }
        block->cpu(&run);
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

#ifdef MN_GPU_SYSTEMS
/* Reads MIDNIGHT_GPU, chooses the device and prints the `info` line. */
static void gpu_start(void) {
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

    const char *device = NULL;
    mn_gpu_rows = gpu_options.rows;
#ifdef MN_GPU
    if (!gpu_options.off) device = mn_gpu_device_start(gpu_options.software);
#endif
    if (gpu_options.info) {
        fprintf(stderr, "midnight: gpu: %s\n", device ? device : "no device");
    }
#ifdef MN_GPU
    if (device && mn_gpu_check) gpu_strict_check(device);
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
        free(scratch->outer);
        free(scratch->inner);
        free(scratch->snapshots);
        free(scratch->snapshot_sizes);
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
#ifdef MN_GPU_SYSTEMS
    gpu_start();
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
    gpu_stats_line();
    fflush(stdout);
    gpu_finish();
#ifdef MN_DEBUG
    world_destroy();
    if (mn_live_buffers != 0) {
        fprintf(stderr, "midnight: %lld buffer%s still alive at exit\n",
                (long long)mn_live_buffers, mn_live_buffers == 1 ? "" : "s");
        mn_leave(70);
    }
#endif
}
