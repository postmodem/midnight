/*
 * The Gremlin runtime. See gremlin_rt.h for what each function promises.
 */

#define _POSIX_C_SOURCE 200809L

#include "gremlin_rt.h"

#include <errno.h>
#include <math.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

#define GR_NONE UINT32_MAX

GrWorld gr_world;
GrTime gr_time = {0, 1.0 / 60, 0.0};

/* ---- Leaving the program ----------------------------------------------- */

/* Flushes both standard streams and ends the process at once. */
static _Noreturn void gr_leave(int status) {
    fflush(stdout);
    fflush(stderr);
    _Exit(status);
}

_Noreturn void gr_panic(GrLoc loc, const char *format, ...) {
    fflush(stdout);
    fprintf(stderr, "%s:%d: runtime error: ", loc.file, (int)loc.line);
    va_list args;
    va_start(args, format);
    vfprintf(stderr, format, args);
    va_end(args);
    fputc('\n', stderr);
    gr_leave(1);
}

_Noreturn void gr_exit(int64_t status) { gr_leave((int)(status & 0xff)); }

/* ---- Memory -------------------------------------------------------------- */

static _Noreturn void gr_out_of_memory(void) {
    fflush(stdout);
    fputs("gremlin: out of memory\n", stderr);
    abort();
}

static void *gr_malloc(size_t size) {
    void *memory = malloc(size ? size : 1);
    if (!memory) gr_out_of_memory();
    return memory;
}

static void *gr_calloc(size_t count, size_t size) {
    void *memory = calloc(count ? count : 1, size ? size : 1);
    if (!memory) gr_out_of_memory();
    return memory;
}

static void *gr_realloc(void *memory, size_t size) {
    memory = realloc(memory, size ? size : 1);
    if (!memory) gr_out_of_memory();
    return memory;
}

/* a * b, or an out-of-memory exit if the product does not fit. */
static size_t gr_mul(size_t a, size_t b) {
    if (b != 0 && a > SIZE_MAX / b) gr_out_of_memory();
    return a * b;
}

/* The number of live string and list buffers. */
#ifdef GR_DEBUG
static int64_t gr_live_buffers;
#define GR_BUFFER_BORN() ((void)gr_live_buffers++)
#define GR_BUFFER_GONE() ((void)gr_live_buffers--)
#else
#define GR_BUFFER_BORN() ((void)0)
#define GR_BUFFER_GONE() ((void)0)
#endif

/* ---- Entity slots -------------------------------------------------------- */

/* Takes a slot off the free list, or a new one, and returns its handle.
 * The entity is alive and in no table. */
static GrEntity slot_take(void) {
    GrWorld *w = &gr_world;
    uint32_t index = w->free_head;
    if (index != GR_NONE) {
        w->free_head = w->slots[index].row;
    } else {
        if (w->slot_count == w->slot_capacity) {
            if (w->slot_capacity > UINT32_MAX / 2) gr_out_of_memory();
            uint32_t capacity = w->slot_capacity ? w->slot_capacity * 2 : 64;
            w->slots = gr_realloc(w->slots, (size_t)capacity * sizeof(GrSlot));
            w->slot_capacity = capacity;
        }
        index = w->slot_count++;
        w->slots[index].generation = 1;
    }
    GrSlot *slot = &w->slots[index];
    slot->table = GR_NO_TABLE;
    slot->row = 0;
    return (GrEntity){index, slot->generation};
}

/* Ends the life of every handle to this slot and puts it on the free
 * list. */
static void slot_free(uint32_t index) {
    GrWorld *w = &gr_world;
    GrSlot *slot = &w->slots[index];
    if (++slot->generation == 0) slot->generation = 1;
    slot->table = GR_NO_TABLE;
    slot->row = w->free_head;
    w->free_head = index;
}

/* ---- Tables -------------------------------------------------------------- */

/* What `columns` holds for a tag, and for a component while the table has
 * no capacity. Never read or written. */
static _Alignas(16) char gr_no_storage[16];

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
    GrWorld *w = &gr_world;
    uint32_t mask = w->table_map_capacity - 1;
    uint32_t at = (uint32_t)hash & mask;
    while (w->table_map[at] != 0) at = (at + 1) & mask;
    w->table_map[at] = id + 1;
}

/* Creates the table for a component set (ascending ids) and registers
 * it. */
static uint32_t table_create(const uint32_t *set, uint32_t set_count,
                             uint64_t hash) {
    GrWorld *w = &gr_world;
    const GrComponentInfo *info = w->program->components;
    size_t component_count = w->program->component_count;

    /* One block: the table, its columns, then its six id arrays. */
    size_t ints = (size_t)set_count * 4 + component_count * 2;
    char *block = gr_malloc(sizeof(GrTable) +
                            component_count * sizeof(void *) +
                            ints * sizeof(uint32_t));
    GrTable *table = (GrTable *)(void *)block;
    table->columns = (void **)(void *)(block + sizeof(GrTable));
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
    for (size_t c = 0; c < component_count; c++) {
        table->columns[c] = NULL;
        table->add_edges[c] = GR_NONE;
        table->remove_edges[c] = table->id;
    }
    for (uint32_t i = 0; i < set_count; i++) {
        uint32_t c = set[i];
        table->set[i] = c;
        table->columns[c] = gr_no_storage;
        table->add_edges[c] = table->id;
        table->remove_edges[c] = GR_NONE;
        if (info[c].size != 0) {
            table->data[table->data_count] = c;
            table->data_sizes[table->data_count] = info[c].size;
            table->data_count++;
            if (info[c].drop) table->drops[table->drop_count++] = c;
        }
    }

    if (w->table_count == w->table_capacity) {
        if (w->table_capacity > UINT32_MAX / 4) gr_out_of_memory();
        uint32_t capacity = w->table_capacity ? w->table_capacity * 2 : 16;
        w->tables = gr_realloc(w->tables, (size_t)capacity * sizeof(GrTable *));
        w->table_capacity = capacity;
    }
    w->tables[w->table_count++] = table;

    if (w->table_count > w->table_map_capacity / 2) {
        free(w->table_map);
        w->table_map_capacity *= 2;
        w->table_map = gr_calloc(w->table_map_capacity, sizeof(uint32_t));
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
    GrWorld *w = &gr_world;
    uint64_t hash = set_hash(set, set_count);
    uint32_t mask = w->table_map_capacity - 1;
    for (uint32_t at = (uint32_t)hash & mask;; at = (at + 1) & mask) {
        uint32_t entry = w->table_map[at];
        if (entry == 0) break;
        const GrTable *table = w->tables[entry - 1];
        if (table->hash != hash || table->set_count != set_count) continue;
        uint32_t i = 0;
        while (i < set_count && table->set[i] == set[i]) i++;
        if (i == set_count) return entry - 1;
    }
    return table_create(set, set_count, hash);
}

/* The table reached from `table` by adding one component. */
static uint32_t table_with_one(GrTable *table, uint32_t component) {
    GrWorld *w = &gr_world;
    uint32_t to = table->add_edges[component];
    if (to != GR_NONE) return to;
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
static uint32_t table_with(GrTable *table, const uint32_t *components,
                           uint32_t count) {
    if (count == 1) return table_with_one(table, components[0]);
    uint32_t *set = gr_world.scratch_set;
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
static uint32_t table_without(GrTable *table, const uint32_t *components,
                              uint32_t count) {
    GrWorld *w = &gr_world;
    if (count == 1) {
        uint32_t to = table->remove_edges[components[0]];
        if (to != GR_NONE) return to;
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
static uint32_t bundle_table(GrTable *from, GrBundle *bundle) {
    if (bundle->cached && bundle->cached_from == from->id) {
        return bundle->cached_to;
    }
    uint32_t to = table_with(from, bundle->components, bundle->count);
    bundle->cached_from = from->id;
    bundle->cached_to = to;
    bundle->cached = true;
    return to;
}

static void table_grow(GrTable *table) {
    if (table->capacity > UINT32_MAX / 2) gr_out_of_memory();
    uint32_t capacity = table->capacity ? table->capacity * 2 : 8;
    table->entities =
        gr_realloc(table->entities, (size_t)capacity * sizeof(GrEntity));
    for (uint32_t i = 0; i < table->data_count; i++) {
        uint32_t c = table->data[i];
        void *old = table->capacity ? table->columns[c] : NULL;
        table->columns[c] =
            gr_realloc(old, gr_mul(capacity, table->data_sizes[i]));
    }
    table->capacity = capacity;
}

/* Adds a row for `e` and returns its index. The row's values are not
 * initialised. */
static inline uint32_t table_append(GrTable *table, GrEntity e) {
    if (table->count == table->capacity) table_grow(table);
    uint32_t row = table->count++;
    table->entities[row] = e;
    return row;
}

/* Takes a row out by moving the last row into its place. The values of
 * the row are not dropped. */
static void table_remove(GrTable *table, uint32_t row) {
    uint32_t last = --table->count;
    if (row == last) return;
    for (uint32_t i = 0; i < table->data_count; i++) {
        size_t size = table->data_sizes[i];
        char *column = table->columns[table->data[i]];
        memcpy(column + row * size, column + last * size, size);
    }
    GrEntity moved = table->entities[last];
    table->entities[row] = moved;
    gr_world.slots[moved.index].row = row;
}

/* Drops everything the components of one row own. */
static void table_drop_row(GrTable *table, uint32_t row) {
    const GrComponentInfo *info = gr_world.program->components;
    for (uint32_t i = 0; i < table->drop_count; i++) {
        uint32_t c = table->drops[i];
        info[c].drop((char *)table->columns[c] + (size_t)row * info[c].size);
    }
}

/* ---- Structural changes, applied at once -------------------------------- */

/* Drops the values of a bundle that will not be stored. */
static void bundle_drop(const GrBundle *bundle, void *const *values) {
    const GrComponentInfo *info = gr_world.program->components;
    if (!values) return;
    for (uint32_t i = 0; i < bundle->count; i++) {
        const GrComponentInfo *c = &info[bundle->components[i]];
        if (c->size != 0 && c->drop && values[i]) c->drop(values[i]);
    }
}

/* Puts an entity that is in no table into the bundle's table. */
static void spawn_now(GrEntity e, GrBundle *bundle, void *const *values) {
    GrWorld *w = &gr_world;
    const GrComponentInfo *info = w->program->components;
    uint32_t id = bundle_table(w->tables[0], bundle);
    GrTable *table = w->tables[id];
    uint32_t row = table_append(table, e);
    GrSlot *slot = &w->slots[e.index];
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

static void add_now(GrEntity e, GrBundle *bundle, void *const *values) {
    GrWorld *w = &gr_world;
    const GrComponentInfo *info = w->program->components;
    if (!gr_alive(e) || w->slots[e.index].table == GR_NO_TABLE) {
        bundle_drop(bundle, values);
        return;
    }
    GrSlot *slot = &w->slots[e.index];
    GrTable *from = w->tables[slot->table];
    uint32_t from_row = slot->row;
    uint32_t id = bundle_table(from, bundle);
    GrTable *to = w->tables[id];
    uint32_t to_row = from_row;
    if (to != from) {
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

static void remove_now(GrEntity e, const uint32_t *components,
                       uint32_t count) {
    GrWorld *w = &gr_world;
    const GrComponentInfo *info = w->program->components;
    if (!gr_alive(e) || w->slots[e.index].table == GR_NO_TABLE) return;
    GrSlot *slot = &w->slots[e.index];
    GrTable *from = w->tables[slot->table];
    uint32_t from_row = slot->row;
    uint32_t id = table_without(from, components, count);
    if (id == from->id) return;
    GrTable *to = w->tables[id];
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

static void despawn_now(GrEntity e) {
    GrWorld *w = &gr_world;
    if (!gr_alive(e)) return;
    GrSlot *slot = &w->slots[e.index];
    if (slot->table != GR_NO_TABLE) {
        GrTable *table = w->tables[slot->table];
        if (table->drop_count != 0) table_drop_row(table, slot->row);
        table_remove(table, slot->row);
        w->live_entities--;
    }
    slot_free(e.index);
}

/* ---- Structural changes, queued ----------------------------------------- */

enum { GR_CMD_SPAWN, GR_CMD_ADD, GR_CMD_REMOVE, GR_CMD_DESPAWN };

/* The start of every queued record. A spawn or add record continues with
 * the bundle's address and then each of its values with storage, in bundle
 * order; a remove record continues with `count` component ids. Every part
 * starts at an 8-byte boundary. */
typedef struct GrCommand {
    uint32_t kind;
    uint32_t count;
    GrEntity entity;
} GrCommand;

/* A queue larger than this is released once it has been applied. */
#define GR_COMMANDS_KEPT ((size_t)1 << 20)

static size_t round8(size_t size) { return (size + 7) & ~(size_t)7; }

/* Appends a record with `payload` bytes after its start and returns the
 * address of the payload. */
static unsigned char *command_push(uint32_t kind, uint32_t count, GrEntity e,
                                   size_t payload) {
    GrWorld *w = &gr_world;
    size_t need = sizeof(GrCommand) + payload;
    if (w->command_capacity - w->command_len < need) {
        size_t capacity = w->command_capacity ? w->command_capacity : 1024;
        while (capacity - w->command_len < need) {
            if (capacity > SIZE_MAX / 2) gr_out_of_memory();
            capacity *= 2;
        }
        w->commands = gr_realloc(w->commands, capacity);
        w->command_capacity = capacity;
    }
    unsigned char *at = w->commands + w->command_len;
    GrCommand command = {kind, count, e};
    memcpy(at, &command, sizeof command);
    w->command_len += need;
    w->deferred_count++;
    return at + sizeof command;
}

/* Queues a spawn or an add, copying the values in. */
static void command_push_bundle(uint32_t kind, GrEntity e, GrBundle *bundle,
                                void *const *values) {
    const GrComponentInfo *info = gr_world.program->components;
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

/* Goes through the queue in order and empties it. With `apply`, each
 * change is made; without, the queued values are only dropped. */
static void commands_run(bool apply) {
    GrWorld *w = &gr_world;
    const GrComponentInfo *info = w->program->components;
    unsigned char *at = w->commands;
    unsigned char *end = at + w->command_len;
    while (at < end) {
        GrCommand command;
        memcpy(&command, at, sizeof command);
        at += sizeof command;
        GrEntity e = command.entity;
        switch (command.kind) {
        case GR_CMD_SPAWN:
        case GR_CMD_ADD: {
            GrBundle *bundle;
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
            } else if (command.kind == GR_CMD_ADD) {
                add_now(e, bundle, values);
            } else if (gr_alive(e) &&
                       w->slots[e.index].table == GR_NO_TABLE) {
                spawn_now(e, bundle, values);
            } else {
                bundle_drop(bundle, values);
            }
            break;
        }
        case GR_CMD_REMOVE:
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
    if (w->command_capacity > GR_COMMANDS_KEPT) {
        free(w->commands);
        w->commands = NULL;
        w->command_capacity = 0;
    }
}

void gr_flush(void) { commands_run(true); }

GrEntity gr_spawn(GrBundle *bundle, void *const *values) {
    GrEntity e = slot_take();
    if (gr_world.defer_depth != 0) {
        command_push_bundle(GR_CMD_SPAWN, e, bundle, values);
    } else {
        spawn_now(e, bundle, values);
    }
    return e;
}

void gr_add(GrEntity e, GrBundle *bundle, void *const *values) {
    if (gr_world.defer_depth == 0) {
        add_now(e, bundle, values);
    } else if (gr_alive(e)) {
        command_push_bundle(GR_CMD_ADD, e, bundle, values);
    } else {
        bundle_drop(bundle, values);
    }
}

void gr_remove(GrEntity e, const uint32_t *components, uint32_t count) {
    if (gr_world.defer_depth == 0) {
        remove_now(e, components, count);
    } else if (gr_alive(e) && count != 0) {
        size_t bytes = (size_t)count * sizeof(uint32_t);
        unsigned char *at =
            command_push(GR_CMD_REMOVE, count, e, round8(bytes));
        memcpy(at, components, bytes);
    }
}

void gr_despawn(GrEntity e) {
    if (gr_world.defer_depth == 0) {
        despawn_now(e);
    } else if (gr_alive(e)) {
        command_push(GR_CMD_DESPAWN, 0, e, 0);
    }
}

/* ---- Queries ------------------------------------------------------------- */

void gr_query_refresh(GrQuery *query) {
    GrWorld *w = &gr_world;
    for (uint32_t t = query->tables_seen; t < w->table_count; t++) {
        GrTable *table = w->tables[t];
        bool matches = true;
        for (uint32_t i = 0; matches && i < query->with_count; i++) {
            matches = table->columns[query->with[i]] != NULL;
        }
        for (uint32_t i = 0; matches && i < query->without_count; i++) {
            matches = table->columns[query->without[i]] == NULL;
        }
        if (!matches) continue;
        if (query->table_count == query->table_capacity) {
            if (query->table_capacity > UINT32_MAX / 2) gr_out_of_memory();
            uint32_t capacity =
                query->table_capacity ? query->table_capacity * 2 : 4;
            query->tables = gr_realloc(query->tables,
                                       (size_t)capacity * sizeof(GrTable *));
            query->table_capacity = capacity;
        }
        query->tables[query->table_count++] = table;
    }
    query->tables_seen = w->table_count;
}

int64_t gr_query_count(GrQuery *query) {
    gr_query_update(query);
    int64_t count = 0;
    for (uint32_t i = 0; i < query->table_count; i++) {
        count += query->tables[i]->count;
    }
    return count;
}

int64_t gr_entity_count(void) { return gr_world.live_entities; }

/* ---- Events -------------------------------------------------------------- */

/* The events of one type that have not been discarded: those numbered
 * [base, end), stored from the start of `data`. */
typedef struct GrEventBuf {
    char *data;
    uint64_t base;
    uint64_t end;
    /* `end` as it was when the previous tick finished. */
    uint64_t mark;
    uint64_t capacity; /* in events */
} GrEventBuf;

void *gr_emit(uint32_t event) {
    GrEventBuf *buf = &gr_world.events[event];
    size_t size = gr_world.program->events[event].size;
    uint64_t count = buf->end - buf->base;
    if (count == buf->capacity) {
        if (buf->capacity > SIZE_MAX / 2) gr_out_of_memory();
        uint64_t capacity = buf->capacity ? buf->capacity * 2 : 16;
        buf->data = gr_realloc(buf->data, gr_mul((size_t)capacity, size));
        buf->capacity = capacity;
    }
    void *slot = buf->data + count * size;
    memset(slot, 0, size);
    buf->end++;
    return slot;
}

uint64_t gr_event_end(uint32_t event) { return gr_world.events[event].end; }

void *gr_event_at(uint32_t event, uint64_t seq) {
    const GrEventBuf *buf = &gr_world.events[event];
    if (seq < buf->base || seq >= buf->end) return NULL;
    return buf->data + (seq - buf->base) * gr_world.program->events[event].size;
}

/* Drops and removes the events of one type numbered below `until`. */
static void events_discard(uint32_t event, uint64_t until) {
    GrEventBuf *buf = &gr_world.events[event];
    const GrEventInfo *info = &gr_world.program->events[event];
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

static void tick(void) {
    GrWorld *w = &gr_world;
    const GrProgram *program = w->program;
    w->in_tick = true;
    for (uint32_t i = 0; i < program->system_count; i++) program->systems[i]();
    w->in_tick = false;
    gr_time.tick++;
    gr_time.elapsed += gr_time.dt;
    for (uint32_t i = 0; i < program->event_count; i++) {
        events_discard(i, w->events[i].mark);
        w->events[i].mark = w->events[i].end;
    }
}

void gr_tick(GrLoc loc) {
    if (gr_world.in_tick) {
        gr_panic(loc, "World.tick called while a tick is in progress");
    }
    tick();
}

void gr_run(GrLoc loc) {
    if (gr_world.in_tick) {
        gr_panic(loc, "World.run called while a tick is in progress");
    }
    gr_world.stop_requested = false;
    do {
        tick();
    } while (!gr_world.stop_requested);
}

void gr_stop(void) { gr_world.stop_requested = true; }

/* ---- Text forms ---------------------------------------------------------- */

/* Each writes at `out` and returns the number of bytes written. */

enum { GR_INT_TEXT = 20, GR_FLOAT_TEXT = 32, GR_ENTITY_TEXT = 32 };

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

static int format_entity(GrEntity e, char *out) {
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

_Noreturn void gr_get_failed(GrEntity e, uint32_t component, GrLoc loc) {
    char entity[GR_ENTITY_TEXT];
    int len = format_entity(e, entity);
    if (gr_alive(e)) {
        gr_panic(loc, "%.*s has no %s", len, entity,
                 gr_world.program->components[component].name);
    }
    gr_panic(loc, "%.*s is not alive", len, entity);
}

_Noreturn void gr_index_failed(int64_t index, int64_t len, GrLoc loc) {
    gr_panic(loc, "index %lld out of range for a list of length %lld",
             (long long)index, (long long)len);
}

_Noreturn void gr_div_failed(const char *what, GrLoc loc) {
    gr_panic(loc, "%s by zero", what);
}

_Noreturn void gr_range_failed(int64_t value, const char *type, GrLoc loc) {
    gr_panic(loc, "%lld does not fit in %s", (long long)value, type);
}

/* ---- Strings ------------------------------------------------------------- */

/* A new buffer for `len` bytes (above zero), which the caller fills in.
 * A NUL follows the bytes. */
static GrString str_alloc(int64_t len) {
    if ((uint64_t)len > SIZE_MAX - sizeof(GrStr) - 1) gr_out_of_memory();
    GrStr *s = gr_malloc(sizeof(GrStr) + (size_t)len + 1);
    s->refs = 1;
    s->len = len;
    s->bytes[len] = '\0';
    GR_BUFFER_BORN();
    return s;
}

void gr_str_free(GrString s) {
    if (!s || s->refs < 0) return;
    free(s);
    GR_BUFFER_GONE();
}

GrString gr_str_new(const char *bytes, int64_t len) {
    if (len <= 0) return NULL;
    GrString s = str_alloc(len);
    memcpy(s->bytes, bytes, (size_t)len);
    return s;
}

GrString gr_str_concat(GrString a, GrString b) {
    if (!a) {
        gr_str_retain(b);
        return b;
    }
    if (!b) {
        gr_str_retain(a);
        return a;
    }
    if (a->len > INT64_MAX - b->len) gr_out_of_memory();
    GrString s = str_alloc(a->len + b->len);
    memcpy(s->bytes, a->bytes, (size_t)a->len);
    memcpy(s->bytes + a->len, b->bytes, (size_t)b->len);
    return s;
}

GrString gr_str_repeat(GrString s, int64_t n) {
    if (!s || n <= 0) return NULL;
    if (n == 1) {
        gr_str_retain(s);
        return s;
    }
    if (s->len > INT64_MAX / n) gr_out_of_memory();
    GrString result = str_alloc(s->len * n);
    for (int64_t i = 0; i < n; i++) {
        memcpy(result->bytes + i * s->len, s->bytes, (size_t)s->len);
    }
    return result;
}

bool gr_str_eq(GrString a, GrString b) {
    if (a == b) return true;
    int64_t len = gr_str_bytesize(a);
    if (len != gr_str_bytesize(b)) return false;
    return len == 0 || memcmp(a->bytes, b->bytes, (size_t)len) == 0;
}

int gr_str_cmp(GrString a, GrString b) {
    int64_t a_len = gr_str_bytesize(a);
    int64_t b_len = gr_str_bytesize(b);
    int64_t shared = a_len < b_len ? a_len : b_len;
    if (shared != 0) {
        int order = memcmp(a->bytes, b->bytes, (size_t)shared);
        if (order != 0) return order < 0 ? -1 : 1;
    }
    return (a_len > b_len) - (a_len < b_len);
}

int64_t gr_str_length(GrString s) {
    if (!s) return 0;
    int64_t count = 0;
    for (int64_t i = 0; i < s->len; i++) {
        count += ((unsigned char)s->bytes[i] & 0xc0) != 0x80;
    }
    return count;
}

/* Copies `s` with the ASCII letters in [first, first + 26) moved by
 * `shift`. Returns `s` itself when it has none. */
static GrString str_shift_case(GrString s, char first, int shift) {
    if (!s) return NULL;
    int64_t i = 0;
    while (i < s->len && !(s->bytes[i] >= first && s->bytes[i] < first + 26)) {
        i++;
    }
    if (i == s->len) {
        gr_str_retain(s);
        return s;
    }
    GrString result = str_alloc(s->len);
    for (i = 0; i < s->len; i++) {
        char byte = s->bytes[i];
        if (byte >= first && byte < first + 26) byte = (char)(byte + shift);
        result->bytes[i] = byte;
    }
    return result;
}

GrString gr_str_upcase(GrString s) { return str_shift_case(s, 'a', 'A' - 'a'); }

GrString gr_str_downcase(GrString s) {
    return str_shift_case(s, 'A', 'a' - 'A');
}

bool gr_str_include(GrString s, GrString part) {
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

bool gr_str_start_with(GrString s, GrString prefix) {
    if (!prefix) return true;
    if (!s || prefix->len > s->len) return false;
    return memcmp(s->bytes, prefix->bytes, (size_t)prefix->len) == 0;
}

bool gr_str_end_with(GrString s, GrString suffix) {
    if (!suffix) return true;
    if (!s || suffix->len > s->len) return false;
    return memcmp(s->bytes + (s->len - suffix->len), suffix->bytes,
                  (size_t)suffix->len) == 0;
}

/* `s` with spaces after it (`after`) or before it, up to `width` code
 * points. */
static GrString str_pad(GrString s, int64_t width, bool after) {
    int64_t length = gr_str_length(s);
    if (width <= length) {
        gr_str_retain(s);
        return s;
    }
    int64_t pad = width - length;
    int64_t len = gr_str_bytesize(s);
    if (pad > INT64_MAX - len) gr_out_of_memory();
    GrString result = str_alloc(len + pad);
    char *text = result->bytes + (after ? 0 : pad);
    char *spaces = result->bytes + (after ? len : 0);
    if (len != 0) memcpy(text, s->bytes, (size_t)len);
    memset(spaces, ' ', (size_t)pad);
    return result;
}

GrString gr_str_ljust(GrString s, int64_t width) {
    return str_pad(s, width, true);
}

GrString gr_str_rjust(GrString s, int64_t width) {
    return str_pad(s, width, false);
}

static bool is_space(char c) {
    return c == ' ' || (c >= '\t' && c <= '\r');
}

static bool is_digit(char c) { return c >= '0' && c <= '9'; }

int64_t gr_str_to_i(GrString s) {
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

double gr_str_to_f(GrString s) {
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

GrString gr_int_to_s(int64_t value) {
    char text[GR_INT_TEXT];
    return gr_str_new(text, format_int(value, text));
}

GrString gr_float_to_s(double value) {
    char text[GR_FLOAT_TEXT];
    return gr_str_new(text, format_float(value, text));
}

GR_STR_LIT(gr_text_true, 4, "true");
GR_STR_LIT(gr_text_false, 5, "false");

GrString gr_bool_to_s(bool value) {
    return value ? (GrString)(void *)&gr_text_true
                 : (GrString)(void *)&gr_text_false;
}

GrString gr_entity_to_s(GrEntity e) {
    char text[GR_ENTITY_TEXT];
    return gr_str_new(text, format_entity(e, text));
}

/* ---- String builder ------------------------------------------------------ */

/* Makes room for `extra` more bytes and returns where they go. */
static char *sb_reserve(GrBuilder *sb, int64_t extra) {
    if (sb->capacity - sb->len < extra) {
        int64_t capacity = sb->capacity ? sb->capacity : 64;
        while (capacity - sb->len < extra) {
            if (capacity > INT64_MAX / 2) gr_out_of_memory();
            capacity *= 2;
        }
        sb->bytes = gr_realloc(sb->bytes, (size_t)capacity);
        sb->capacity = capacity;
    }
    return sb->bytes + sb->len;
}

void gr_sb_bytes(GrBuilder *sb, const char *bytes, int64_t len) {
    if (len <= 0) return;
    memcpy(sb_reserve(sb, len), bytes, (size_t)len);
    sb->len += len;
}

void gr_sb_str(GrBuilder *sb, GrString s) {
    if (s) gr_sb_bytes(sb, s->bytes, s->len);
}

void gr_sb_quoted(GrBuilder *sb, GrString s) {
    static const char hex[] = "0123456789ABCDEF";
    int64_t len = gr_str_bytesize(s);
    gr_sb_bytes(sb, "\"", 1);
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
        gr_sb_bytes(sb, s->bytes + plain, i - plain);
        gr_sb_bytes(sb, escape, escape_len);
        plain = i + 1;
    }
    if (len != 0) gr_sb_bytes(sb, s->bytes + plain, len - plain);
    gr_sb_bytes(sb, "\"", 1);
}

void gr_sb_int(GrBuilder *sb, int64_t value) {
    sb->len += format_int(value, sb_reserve(sb, GR_INT_TEXT));
}

void gr_sb_float(GrBuilder *sb, double value) {
    sb->len += format_float(value, sb_reserve(sb, GR_FLOAT_TEXT));
}

void gr_sb_bool(GrBuilder *sb, bool value) {
    if (value) {
        gr_sb_bytes(sb, "true", 4);
    } else {
        gr_sb_bytes(sb, "false", 5);
    }
}

void gr_sb_entity(GrBuilder *sb, GrEntity e) {
    sb->len += format_entity(e, sb_reserve(sb, GR_ENTITY_TEXT));
}

static void sb_reset(GrBuilder *sb) {
    free(sb->bytes);
    sb->bytes = NULL;
    sb->len = 0;
    sb->capacity = 0;
}

GrString gr_sb_finish(GrBuilder *sb) {
    GrString s = gr_str_new(sb->bytes, sb->len);
    sb_reset(sb);
    return s;
}

void gr_sb_write(GrBuilder *sb) {
    gr_out_bytes(sb->bytes, sb->len);
    sb_reset(sb);
}

/* ---- Output -------------------------------------------------------------- */

void gr_out_bytes(const char *bytes, int64_t len) {
    if (len > 0) fwrite(bytes, 1, (size_t)len, stdout);
}

void gr_out_str(GrString s) {
    if (s) gr_out_bytes(s->bytes, s->len);
}

void gr_out_int(int64_t value) {
    char text[GR_INT_TEXT];
    gr_out_bytes(text, format_int(value, text));
}

void gr_out_float(double value) {
    char text[GR_FLOAT_TEXT];
    gr_out_bytes(text, format_float(value, text));
}

void gr_out_bool(bool value) {
    if (value) {
        gr_out_bytes("true", 4);
    } else {
        gr_out_bytes("false", 5);
    }
}

void gr_out_entity(GrEntity e) {
    char text[GR_ENTITY_TEXT];
    gr_out_bytes(text, format_entity(e, text));
}

void gr_out_newline(void) { putc('\n', stdout); }

/* ---- Lists --------------------------------------------------------------- */

static char *list_at(GrList list, const GrElem *elem, int64_t index) {
    return (char *)gr_list_data(list) + (size_t)index * elem->size;
}

static size_t list_bytes(const GrElem *elem, int64_t capacity) {
    if ((uint64_t)capacity > SIZE_MAX) gr_out_of_memory();
    size_t bytes = gr_mul((size_t)capacity, elem->size);
    if (bytes > SIZE_MAX - sizeof(GrListBuf)) gr_out_of_memory();
    return sizeof(GrListBuf) + bytes;
}

/* A new buffer with room for `capacity` elements (above zero) and none in
 * use. */
static GrList list_new(const GrElem *elem, int64_t capacity) {
    GrList list = gr_malloc(list_bytes(elem, capacity));
    list->refs = 1;
    list->len = 0;
    list->cap = capacity;
    GR_BUFFER_BORN();
    return list;
}

/* Appends copies of `count` elements, retaining what they own. The buffer
 * must have room. */
static void list_append_copies(GrList list, const GrElem *elem,
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
    if (capacity > INT64_MAX / 2) gr_out_of_memory();
    return capacity < 2 ? 4 : capacity * 2;
}

void gr_list_free(GrList list, const GrElem *elem) {
    if (!list) return;
    if (elem->drop) {
        for (int64_t i = 0; i < list->len; i++) {
            elem->drop(list_at(list, elem, i));
        }
    }
    free(list);
    GR_BUFFER_GONE();
}

GrList gr_list_alloc(const GrElem *elem, int64_t len) {
    if (len <= 0) return NULL;
    GrList list = list_new(elem, len);
    memset(gr_list_data(list), 0, (size_t)len * elem->size);
    list->len = len;
    return list;
}

void gr_list_unique(GrList *list, const GrElem *elem) {
    GrList shared = *list;
    if (!shared || shared->refs == 1) return;
    GrList own = NULL;
    if (shared->len != 0) {
        own = list_new(elem, shared->len);
        list_append_copies(own, elem, gr_list_data(shared), shared->len);
    }
    shared->refs--;
    *list = own;
}

int64_t gr_list_need(GrList list, const char *what, GrLoc loc) {
    int64_t len = gr_list_len(list);
    if (len == 0) gr_panic(loc, "%s on an empty list", what);
    return len;
}

void *gr_list_push(GrList *list, const GrElem *elem) {
    GrList buf = *list;
    if (!buf) {
        buf = list_new(elem, 4);
    } else if (buf->refs > 1) {
        GrList own = list_new(elem, list_grown(buf->len));
        list_append_copies(own, elem, gr_list_data(buf), buf->len);
        buf->refs--;
        buf = own;
    } else if (buf->len == buf->cap) {
        int64_t capacity = list_grown(buf->cap);
        buf = gr_realloc(buf, list_bytes(elem, capacity));
        buf->cap = capacity;
    }
    *list = buf;
    void *slot = list_at(buf, elem, buf->len);
    memset(slot, 0, elem->size);
    buf->len++;
    return slot;
}

void gr_list_pop(GrList *list, const GrElem *elem, void *out, GrLoc loc) {
    int64_t len = gr_list_need(*list, "pop", loc);
    gr_list_unique(list, elem);
    GrList buf = *list;
    memcpy(out, list_at(buf, elem, len - 1), elem->size);
    buf->len = len - 1;
}

void gr_list_delete_at(GrList *list, const GrElem *elem, int64_t index,
                       void *out, GrLoc loc) {
    int64_t at = gr_list_index(*list, index, loc);
    gr_list_unique(list, elem);
    GrList buf = *list;
    char *slot = list_at(buf, elem, at);
    memcpy(out, slot, elem->size);
    buf->len--;
    memmove(slot, slot + elem->size, (size_t)(buf->len - at) * elem->size);
}

void gr_list_clear(GrList *list, const GrElem *elem) {
    gr_list_release(*list, elem);
    *list = NULL;
}

GrList gr_list_concat(GrList a, GrList b, const GrElem *elem) {
    int64_t a_len = gr_list_len(a);
    int64_t b_len = gr_list_len(b);
    if (a_len == 0 && b_len == 0) return NULL;
    if (b_len == 0) {
        gr_list_retain(a);
        return a;
    }
    if (a_len == 0) {
        gr_list_retain(b);
        return b;
    }
    if (a_len > INT64_MAX - b_len) gr_out_of_memory();
    GrList list = list_new(elem, a_len + b_len);
    list_append_copies(list, elem, gr_list_data(a), a_len);
    list_append_copies(list, elem, gr_list_data(b), b_len);
    return list;
}

GrList gr_list_repeat(GrList list, int64_t n, const GrElem *elem) {
    int64_t len = gr_list_len(list);
    if (len == 0 || n <= 0) return NULL;
    if (n == 1) {
        gr_list_retain(list);
        return list;
    }
    if (len > INT64_MAX / n) gr_out_of_memory();
    GrList result = list_new(elem, len * n);
    for (int64_t i = 0; i < n; i++) {
        list_append_copies(result, elem, gr_list_data(list), len);
    }
    return result;
}

/* ---- Numbers ------------------------------------------------------------- */

int64_t gr_int_pow(int64_t base, int64_t exponent, GrLoc loc) {
    if (exponent < 0) {
        gr_panic(loc, "negative exponent %lld", (long long)exponent);
    }
    uint64_t result = 1;
    uint64_t factor = (uint64_t)base;
    for (uint64_t left = (uint64_t)exponent; left != 0; left >>= 1) {
        if (left & 1) result *= factor;
        factor *= factor;
    }
    return (int64_t)result;
}

double gr_float_mod(double a, double b) {
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

int64_t gr_float_to_i(double value) { return whole_to_int(trunc(value)); }
int64_t gr_float_floor(double value) { return whole_to_int(floor(value)); }
int64_t gr_float_ceil(double value) { return whole_to_int(ceil(value)); }
int64_t gr_float_round(double value) { return whole_to_int(round(value)); }

double gr_float_round_digits(double value, int64_t digits) {
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

#define GR_DEFAULT_SEED 0x6772656d6c696eLL

static uint64_t gr_random[4];

static uint64_t rotate_left(uint64_t value, int by) {
    return (value << by) | (value >> (64 - by));
}

/* xoshiro256** */
static uint64_t random_next(void) {
    uint64_t *s = gr_random;
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

void gr_srand(int64_t seed) {
    /* splitmix64 */
    uint64_t state = (uint64_t)seed;
    for (int i = 0; i < 4; i++) {
        uint64_t z = (state += 0x9e3779b97f4a7c15u);
        z = (z ^ (z >> 30)) * 0xbf58476d1ce4e5b9u;
        z = (z ^ (z >> 27)) * 0x94d049bb133111ebu;
        gr_random[i] = z ^ (z >> 31);
    }
}

void gr_srand_clock(void) {
    struct timespec now = {0, 0};
    clock_gettime(CLOCK_REALTIME, &now);
    gr_srand((int64_t)((uint64_t)now.tv_sec * 1000000000u +
                       (uint64_t)now.tv_nsec));
}

double gr_rand(void) {
    return (double)(random_next() >> 11) * (1.0 / 9007199254740992.0);
}

int64_t gr_rand_int(int64_t n, GrLoc loc) {
    if (n <= 0) {
        gr_panic(loc, "rand(%lld): the argument must be positive",
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

static struct timespec gr_started;

double gr_clock(void) {
    struct timespec now = gr_started;
    clock_gettime(CLOCK_MONOTONIC, &now);
    return (double)(now.tv_sec - gr_started.tv_sec) +
           (double)(now.tv_nsec - gr_started.tv_nsec) * 1e-9;
}

void gr_sleep(double seconds) {
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

/* ---- Start and finish ---------------------------------------------------- */

void gr_start(const GrProgram *program) {
    GrWorld *w = &gr_world;
    memset(w, 0, sizeof *w);
    w->program = program;
    w->free_head = GR_NONE;
    size_t component_count = program->component_count;
    w->scratch_set = gr_malloc((component_count + 1) * sizeof(uint32_t));
    w->scratch_values = gr_malloc((component_count + 1) * sizeof(void *));
    w->events = gr_calloc(program->event_count, sizeof(GrEventBuf));
    w->table_map_capacity = 64;
    w->table_map = gr_calloc(w->table_map_capacity, sizeof(uint32_t));
    table_for_set(w->scratch_set, 0);
    gr_srand(GR_DEFAULT_SEED);
    clock_gettime(CLOCK_MONOTONIC, &gr_started);
}

#ifdef GR_DEBUG
/* Drops every value the world holds and frees the world's memory. */
static void world_destroy(void) {
    GrWorld *w = &gr_world;
    const GrProgram *program = w->program;

    for (uint32_t t = 0; t < w->table_count; t++) {
        GrTable *table = w->tables[t];
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
        GrTable *table = w->tables[t];
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
    w->free_head = GR_NONE;
}
#endif

void gr_finish(void) {
    fflush(stdout);
#ifdef GR_DEBUG
    world_destroy();
    if (gr_live_buffers != 0) {
        fprintf(stderr, "gremlin: %lld buffer%s still alive at exit\n",
                (long long)gr_live_buffers, gr_live_buffers == 1 ? "" : "s");
        gr_leave(70);
    }
#endif
}
