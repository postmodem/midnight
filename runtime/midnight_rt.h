/*
 * The Midnight runtime: everything a compiled program needs that does not
 * depend on the program's own types. Generated code includes this header
 * and is compiled together with midnight_rt.c.
 *
 * Conventions
 *
 * - All-zero bytes are a valid value of every type: 0, 0.0, false, the
 *   empty string (NULL), the empty list (NULL), Entity.none and the first
 *   variant of an enum. The runtime zero-fills storage it hands out.
 * - Strings and lists are pointers to reference-counted buffers. A function
 *   that returns a MnString or MnList returns an owned reference (the caller
 *   must release it or store it). Arguments are borrowed unless a function
 *   says it takes ownership.
 * - Values handed to the world (spawn, add, emit) are moved in: the caller
 *   must not drop them afterwards.
 * - Counts are not atomic. Nothing here is thread-safe.
 * - A MnLoc names the .12am line an operation came from, for run-time
 *   errors.
 *
 * Run-time errors print
 *
 *     FILE:LINE: runtime error: MESSAGE
 *
 * to standard error (after flushing standard output) and exit with status
 * 1. The messages are:
 *
 *     Entity(7v1) has no Health                    mn_get, entity alive
 *     Entity(7v1) is not alive                     mn_get, entity dead
 *     index 5 out of range for a list of length 3  indexing, delete_at
 *     pop on an empty list                         also first, last
 *     division by zero                             Int /
 *     modulo by zero                               Int %
 *     300 does not fit in UInt8                    compact integer stores
 *     negative exponent -2                         Int **
 *     rand(0): the argument must be positive
 *     World.tick called while a tick is in progress      also World.run
 *
 * With MN_DEBUG defined, the runtime counts live string and list buffers.
 * mn_finish then tears the world down and, if any buffer is still alive,
 * prints
 *
 *     midnight: 2 buffers still alive at exit
 *
 * to standard error and exits with status 70.
 */

#ifndef MIDNIGHT_RT_H
#define MIDNIGHT_RT_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

/* ---- Errors ---------------------------------------------------------- */

typedef struct MnLoc {
    const char *file;
    int32_t line;
} MnLoc;

/* Reports a run-time error (printf-style message) and exits with status 1. */
_Noreturn void mn_panic(MnLoc loc, const char *format, ...)
    __attribute__((format(printf, 2, 3), cold));

/* ---- Program description --------------------------------------------- */

/* One per component or tag, indexed by component id. */
typedef struct MnComponentInfo {
    const char *name;
    uint32_t size;  /* 0 for a tag */
    uint32_t align; /* 1 for a tag */
    /* Releases what a value owns; NULL when it owns nothing. */
    void (*drop)(void *value);
} MnComponentInfo;

/* One per event type, indexed by event id. */
typedef struct MnEventInfo {
    const char *name;
    uint32_t size; /* at least 1 */
    uint32_t align;
    void (*drop)(void *value); /* as for components */
} MnEventInfo;

typedef struct MnProgram {
    const MnComponentInfo *components;
    uint32_t component_count;
    const MnEventInfo *events;
    uint32_t event_count;
    /* In the order World.tick runs them. */
    void (*const *systems)(void);
    uint32_t system_count;
    /* Releases what constants and resources own. Called by mn_finish when
     * MN_DEBUG is defined, before the buffer check. */
    void (*drop_globals)(void);
} MnProgram;

/* Call once, first. The program description must outlive the program. */
void mn_start(const MnProgram *program);

/* Call when the top-level statements have finished. Flushes standard
 * output. With MN_DEBUG it also applies nothing further, destroys every
 * entity, event and queued change, calls drop_globals, and exits with
 * status 70 if any string or list buffer is still alive. */
void mn_finish(void);

/* The built-in `exit`: flushes standard output and exits with the low
 * eight bits of status. No teardown and no buffer check. */
_Noreturn void mn_exit(int64_t status);

/* ---- Entities, tables and the world ----------------------------------- */

/* A handle: 64 bits. All-zero is Entity.none. Generations start at 1, so
 * no living entity has generation 0. Compare handles field by field. */
typedef struct MnEntity {
    uint32_t index;
    uint32_t generation;
} MnEntity;

/* Table index of an entity that is alive but in no table yet: one whose
 * spawn is queued. */
#define MN_NO_TABLE UINT32_MAX

/* One per entity index: 12 bytes. */
typedef struct MnSlot {
    uint32_t generation;
    uint32_t table; /* index into mn_world.tables, or MN_NO_TABLE */
    uint32_t row;
} MnSlot;

/* Entities with one set of components. The runtime may add fields after
 * the ones listed here. */
typedef struct MnTable {
    /* Indexed by component id. For a component the table has: its array
     * of `count` values. For a tag the table has: a non-NULL pointer that
     * must not be dereferenced. NULL for anything the table lacks. */
    void **columns;
    /* The handle of each row. */
    MnEntity *entities;
    uint32_t count;
    uint32_t capacity;
    /* Runtime-private fields follow. */
    uint32_t id; /* index into mn_world.tables */
    /* The table's component ids, ascending. */
    uint32_t set_count;
    uint32_t *set;
    /* The ids in `set` that have storage, and the size of each. */
    uint32_t data_count;
    uint32_t *data;
    uint32_t *data_sizes;
    /* The ids in `data` whose values own something. */
    uint32_t drop_count;
    uint32_t *drops;
    uint64_t hash; /* of `set` */
    /* Indexed by component id: the table reached by adding or removing
     * that component, or UINT32_MAX when not looked up yet. */
    uint32_t *add_edges;
    uint32_t *remove_edges;
    /* The table's GPU column copies, or NULL when it has none. */
    struct MnGpuTable *gpu;
} MnTable;

/* The runtime may add fields after the ones listed here. */
typedef struct MnWorld {
    MnSlot *slots;
    uint32_t slot_count;
    /* Tables are never destroyed or reordered while the program runs, and
     * none is created while defer_depth is above zero. */
    MnTable **tables;
    uint32_t table_count;
    /* Nesting depth of `each`. */
    uint32_t defer_depth;
    /* Number of queued structural changes. */
    uint32_t deferred_count;
    const MnProgram *program;
    /* Runtime-private fields follow. */
    uint32_t slot_capacity;
    uint32_t table_capacity;
    /* The most recently freed slot, or UINT32_MAX. A free slot's `row`
     * holds the next free index. */
    uint32_t free_head;
    bool in_tick;
    bool stop_requested;
    /* Entities that are in a table. */
    int64_t live_entities;
    /* Open-addressed map from a component set's hash to table index + 1. */
    uint32_t *table_map;
    uint32_t table_map_capacity;
    /* Queued structural changes: records at 8-byte boundaries. */
    unsigned char *commands;
    size_t command_len;
    size_t command_capacity;
    /* One per event type. */
    struct MnEventBuf *events;
    /* Working space: a component set and one value pointer per component. */
    uint32_t *scratch_set;
    void **scratch_values;
} MnWorld;

extern MnWorld mn_world;

/* `e.alive?` */
static inline bool mn_alive(MnEntity e) {
    return e.index < mn_world.slot_count &&
           mn_world.slots[e.index].generation == e.generation;
}

/* `e.has?(C)`: false for a dead handle and for a queued spawn. */
static inline bool mn_has(MnEntity e, uint32_t component) {
    if (!mn_alive(e)) return false;
    uint32_t table = mn_world.slots[e.index].table;
    return table != MN_NO_TABLE &&
           mn_world.tables[table]->columns[component] != NULL;
}

_Noreturn void mn_get_failed(MnEntity e, uint32_t component, MnLoc loc)
    __attribute__((cold));

/* `e[C]`: the address of the entity's component, whose size the caller
 * passes. A run-time error if the entity is dead or lacks the component.
 * The address is valid only until the next structural change, tick or call
 * into user code; do not keep it. Never called for a tag. */
static inline void *mn_get(MnEntity e, uint32_t component, size_t size,
                           MnLoc loc) {
    if (mn_alive(e)) {
        const MnSlot *slot = &mn_world.slots[e.index];
        if (slot->table != MN_NO_TABLE) {
            char *column = mn_world.tables[slot->table]->columns[component];
            if (column) return column + (size_t)slot->row * size;
        }
    }
    mn_get_failed(e, component, loc);
}

/* The array of a component in a table, or NULL. Ask again at the start of
 * each table's loop; do not keep the address across a structural change. */
static inline void *mn_column(const MnTable *table, uint32_t component) {
    return table->columns[component];
}

/* ---- Queries ----------------------------------------------------------- */

/* The tables one `each` or `World.count` matches. Generated code keeps one
 * static MnQuery per use, initialised with its component lists and zeros
 * elsewhere:
 *
 *     static const uint32_t q0_with[] = {0, 1};
 *     static const uint32_t q0_without[] = {2};
 *     static MnQuery q0 = {q0_with, 2, q0_without, 1, 0, 0, 0, 0};
 */
typedef struct MnQuery {
    /* Every component the entity must have: the terms, then `with:`. */
    const uint32_t *with;
    uint32_t with_count;
    const uint32_t *without;
    uint32_t without_count;
    /* Matching tables, in the order they were created. Filled by
     * mn_query_update. Tables with no rows are included; skip them. */
    MnTable **tables;
    uint32_t table_count;
    uint32_t table_capacity;
    /* How many of the world's tables have been looked at. */
    uint32_t tables_seen;
} MnQuery;

void mn_query_refresh(MnQuery *query);

/* Brings `tables` up to date. Call before each use of a query. */
static inline void mn_query_update(MnQuery *query) {
    if (query->tables_seen != mn_world.table_count) mn_query_refresh(query);
}

/* `World.count(…)`: updates the query and adds up its tables' row counts. */
int64_t mn_query_count(MnQuery *query);

/* `World.count`: entities that are in a table. */
int64_t mn_entity_count(void);

/* ---- Structural changes ------------------------------------------------- */

/* While mn_world.defer_depth is above zero, mn_spawn, mn_add, mn_remove and
 * mn_despawn queue their work (taking ownership of the values at once) and
 * the world does not change. mn_defer_leave applies the queue, in order,
 * when the depth returns to zero. Generated code calls mn_defer_enter
 * before the loops of an `each` and mn_defer_leave on every way out. */
void mn_flush(void);

static inline void mn_defer_enter(void) { mn_world.defer_depth++; }

static inline void mn_defer_leave(void) {
    if (--mn_world.defer_depth == 0 && mn_world.deferred_count != 0) mn_flush();
}

/* The component list of one `spawn` or `add` in the source. Generated code
 * keeps one static MnBundle per use, initialised with its list and zeros
 * elsewhere; the runtime caches what it likes in the other fields. No
 * component is listed twice. */
typedef struct MnBundle {
    const uint32_t *components;
    uint32_t count;
    /* Runtime-private: the table this bundle last led to and the table it
     * started from (table 0 for a spawn). */
    uint32_t cached_from;
    uint32_t cached_to;
    bool cached;
} MnBundle;

/* `spawn`: values[i] points at the value for components[i] and is ignored
 * for a tag; `values` may be NULL when the bundle has no component with
 * data. The values are moved in. When deferred, the returned handle is
 * alive but in no table until the queue is applied. */
MnEntity mn_spawn(MnBundle *bundle, void *const *values);

/* `e.add(…)`: as mn_spawn. A component the entity already has is replaced
 * (the old value is dropped). If the entity is dead, the values are dropped
 * and nothing else happens. */
void mn_add(MnEntity e, MnBundle *bundle, void *const *values);

/* `e.remove(…)`: absent components and dead entities are ignored. */
void mn_remove(MnEntity e, const uint32_t *components, uint32_t count);

/* `e.despawn`: nothing happens if the entity is dead. */
void mn_despawn(MnEntity e);

/* ---- Systems, ticks and time ------------------------------------------- */

typedef struct MnTime {
    int64_t tick;   /* ticks completed */
    double dt;      /* starts at 1.0 / 60 */
    double elapsed; /* simulated seconds completed */
} MnTime;

extern MnTime mn_time;

/* `World.tick`: runs every system once, then adds one to mn_time.tick and
 * dt to mn_time.elapsed, then discards events emitted before the end of
 * the previous tick. A run-time error if a tick is in progress. */
void mn_tick(MnLoc loc);

/* `World.run`: clears the stop flag, then ticks until a tick has finished
 * during which mn_stop was called. */
void mn_run(MnLoc loc);

/* `World.stop` */
void mn_stop(void);

/* ---- Events -------------------------------------------------------------- */

/* Every event of a type has a sequence number, counting from 0 in the
 * order emitted. An `on` in the source keeps a static cursor, the sequence
 * number it has read up to, starting at 0:
 *
 *     uint64_t end = mn_event_end(EV);
 *     uint64_t seq = cursor;
 *     cursor = end;
 *     for (; seq < end; seq++) {
 *         void *event = mn_event_at(EV, seq);
 *         if (!event) continue;
 *         ...copy the value out, then run the body...
 *     }
 */

/* `emit`: appends a zeroed slot for one event and returns it. The caller
 * moves the value in at once. */
void *mn_emit(uint32_t event);

/* One past the sequence number of the newest event of this type. */
uint64_t mn_event_end(uint32_t event);

/* The event with this sequence number, or NULL if it has been discarded.
 * The address is valid until the next emit or tick: copy the value out
 * before running any user code, and ask again for the next one. */
void *mn_event_at(uint32_t event, uint64_t seq);

/* ---- Strings ------------------------------------------------------------- */

/* An immutable byte string. NULL is the empty string; no buffer of length
 * zero exists. `refs` below zero marks a static literal that is never
 * counted or freed. */
typedef struct MnStr {
    int64_t refs;
    int64_t len;
    char bytes[];
} MnStr;

typedef MnStr *MnString;

/* Defines a static literal `name`, usable as `(MnString)&name`:
 *
 *     MN_STR_LIT(s0, 13, "Hello, world!");
 *
 * `len` is the byte length, which the C literal's terminating NUL follows.
 * Do not define a literal of length zero; use NULL. */
#define MN_STR_LIT(name, length, text)                                         \
    static struct {                                                            \
        int64_t refs;                                                          \
        int64_t len;                                                           \
        char bytes[(length) + 1];                                              \
    } name = {-1, (length), text}

void mn_str_free(MnString s);

static inline void mn_str_retain(MnString s) {
    if (s && s->refs > 0) s->refs++;
}

static inline void mn_str_release(MnString s) {
    if (s && s->refs > 0 && --s->refs == 0) mn_str_free(s);
}

static inline int64_t mn_str_bytesize(MnString s) { return s ? s->len : 0; }

/* A new string holding `len` bytes copied from `bytes`. */
MnString mn_str_new(const char *bytes, int64_t len);

MnString mn_str_concat(MnString a, MnString b);
/* `s * n`: empty when n <= 0. */
MnString mn_str_repeat(MnString s, int64_t n);
bool mn_str_eq(MnString a, MnString b);
/* Byte-wise order: negative, zero or positive. */
int mn_str_cmp(MnString a, MnString b);
/* Number of Unicode code points. */
int64_t mn_str_length(MnString s);
/* ASCII letters only. */
MnString mn_str_upcase(MnString s);
MnString mn_str_downcase(MnString s);
bool mn_str_include(MnString s, MnString part);
bool mn_str_start_with(MnString s, MnString prefix);
bool mn_str_end_with(MnString s, MnString suffix);
/* Padded with spaces to at least `width` code points. */
MnString mn_str_ljust(MnString s, int64_t width);
MnString mn_str_rjust(MnString s, int64_t width);
/* The number at the start of the string, after optional whitespace, or
 * zero. mn_str_to_i reads an optional sign and decimal digits (underscores
 * between digits allowed) and wraps on overflow. mn_str_to_f reads what
 * strtod reads, except hexadecimal, infinity and NaN. */
int64_t mn_str_to_i(MnString s);
double mn_str_to_f(MnString s);

/* `to_s` of the built-in types. */
MnString mn_int_to_s(int64_t value);
MnString mn_float_to_s(double value);
MnString mn_bool_to_s(bool value);
MnString mn_entity_to_s(MnEntity e);

/* ---- String builder ----------------------------------------------------- */

/* Accumulates text for interpolation, `to_s` and output. Zero-initialise
 * it; then append; then end with exactly one of mn_sb_finish or
 * mn_sb_write. Its storage is not a counted buffer.
 *
 * Text forms:
 *   Int      42
 *   Float    the shortest decimal digits that read back as the same
 *            double, printed positionally with at least one fractional
 *            digit when the decimal exponent is in [-5, 16) (1.0, 0.1,
 *            100.0, 1234567.0, 0.00001) and otherwise in exponent form as
 *            C's %g writes it (1e+20, 1.5e-07); also Infinity, -Infinity
 *            and NaN.
 *   Bool     true, false
 *   Entity   Entity(3v1): index, `v`, generation. Entity(none) when both
 *            are zero.
 *   quoted   "text" in double quotes, with \n \t \r \0 \e \\ \" and \#
 *            (only before `{`) escaped as the language writes them, and
 *            other bytes below 0x20 as \xNN.
 */
typedef struct MnBuilder {
    char *bytes;
    int64_t len;
    int64_t capacity;
} MnBuilder;

void mn_sb_bytes(MnBuilder *sb, const char *bytes, int64_t len);
void mn_sb_str(MnBuilder *sb, MnString s);
void mn_sb_quoted(MnBuilder *sb, MnString s);
void mn_sb_int(MnBuilder *sb, int64_t value);
void mn_sb_float(MnBuilder *sb, double value);
void mn_sb_bool(MnBuilder *sb, bool value);
void mn_sb_entity(MnBuilder *sb, MnEntity e);

/* Returns the text as a new string and frees the builder's storage. */
MnString mn_sb_finish(MnBuilder *sb);
/* Writes the text to standard output and frees the builder's storage. */
void mn_sb_write(MnBuilder *sb);

/* ---- Output -------------------------------------------------------------- */

/* Standard output is buffered; the runtime flushes it before a run-time
 * error and at exit. */
void mn_out_bytes(const char *bytes, int64_t len);
void mn_out_str(MnString s);
void mn_out_int(int64_t value);
void mn_out_float(double value);
void mn_out_bool(bool value);
void mn_out_entity(MnEntity e);
void mn_out_newline(void);

/* ---- Lists ---------------------------------------------------------------- */

/* A list is a pointer to a counted buffer: this header, then `cap`
 * elements of which `len` are in use. NULL is the empty list. Elements
 * are aligned to at most 8 bytes. A buffer whose count is above one is
 * shared and must not be written; mn_list_unique makes a private copy. */
typedef struct MnListBuf {
    int64_t refs;
    int64_t len;
    int64_t cap;
} MnListBuf;

typedef MnListBuf *MnList;

/* How to handle one element type. Generated code keeps one static const
 * MnElem per list type. */
typedef struct MnElem {
    uint32_t size;
    /* Adds a reference to everything the element at `value` owns, after
     * its bytes have been copied. NULL when it owns nothing. */
    void (*retain)(void *value);
    /* Releases what the element owns. NULL when it owns nothing. */
    void (*drop)(void *value);
} MnElem;

static inline int64_t mn_list_len(MnList list) { return list ? list->len : 0; }

/* The first element. Only for a non-NULL list. */
static inline void *mn_list_data(MnList list) { return (void *)(list + 1); }

void mn_list_free(MnList list, const MnElem *elem);

static inline void mn_list_retain(MnList list) {
    if (list) list->refs++;
}

/* Drops the elements and frees the buffer when the last reference goes. */
static inline void mn_list_release(MnList list, const MnElem *elem) {
    if (list && --list->refs == 0) mn_list_free(list, elem);
}

/* A new list of `len` zeroed elements; NULL when len <= 0. The caller
 * fills the elements in. */
MnList mn_list_alloc(const MnElem *elem, int64_t len);

/* Makes `*list` safe to write: if its buffer is shared, replaces it with a
 * private copy (retaining each element) and releases the shared one. */
void mn_list_unique(MnList *list, const MnElem *elem);

_Noreturn void mn_index_failed(int64_t index, int64_t len, MnLoc loc)
    __attribute__((cold));

/* Checks an index and returns it counted from the start. A negative index
 * counts from the end. */
static inline int64_t mn_list_index(MnList list, int64_t index, MnLoc loc) {
    int64_t len = mn_list_len(list);
    int64_t i = index < 0 ? index + len : index;
    if (i < 0 || i >= len) mn_index_failed(index, len, loc);
    return i;
}

/* For `first`, `last` and `pop`: a run-time error naming `what` if the
 * list is empty, otherwise its length. */
int64_t mn_list_need(MnList list, const char *what, MnLoc loc);

/* `push`: makes the list unique, grows it by doubling when full, and
 * returns the new last element, zeroed, for the caller to move a value
 * into. */
void *mn_list_push(MnList *list, const MnElem *elem);

/* `pop`: moves the last element into `out` (the caller owns it). A
 * run-time error on an empty list. A list that becomes empty may keep its
 * buffer. */
void mn_list_pop(MnList *list, const MnElem *elem, void *out, MnLoc loc);

/* `delete_at`: moves the element at `index` (checked as in mn_list_index)
 * into `out` and closes the gap. */
void mn_list_delete_at(MnList *list, const MnElem *elem, int64_t index,
                       void *out, MnLoc loc);

/* `clear`: releases the buffer and sets `*list` to NULL. */
void mn_list_clear(MnList *list, const MnElem *elem);

/* `a + b` and `a * n`: a new list (or a retained operand when the result
 * would equal it). Elements are retained. */
MnList mn_list_concat(MnList a, MnList b, const MnElem *elem);
MnList mn_list_repeat(MnList list, int64_t n, const MnElem *elem);

/* ---- Numbers ---------------------------------------------------------------- */

_Noreturn void mn_div_failed(const char *what, MnLoc loc) __attribute__((cold));
_Noreturn void mn_range_failed(int64_t value, const char *type, MnLoc loc)
    __attribute__((cold));

/* Int `/`: rounds toward negative infinity; wraps for INT64_MIN / -1. */
static inline int64_t mn_int_div(int64_t a, int64_t b, MnLoc loc) {
    if (b == 0) mn_div_failed("division", loc);
    if (b == -1) return (int64_t)(0 - (uint64_t)a);
    int64_t q = a / b;
    return (a % b != 0 && ((a < 0) != (b < 0))) ? q - 1 : q;
}

/* Int `%`: the result has the sign of the divisor. */
static inline int64_t mn_int_mod(int64_t a, int64_t b, MnLoc loc) {
    if (b == 0) mn_div_failed("modulo", loc);
    if (b == -1) return 0;
    int64_t r = a % b;
    return (r != 0 && ((r < 0) != (b < 0))) ? r + b : r;
}

/* Int `**`: wraps on overflow. A run-time error for a negative exponent. */
int64_t mn_int_pow(int64_t base, int64_t exponent, MnLoc loc);

/* Float `%`: the result has the sign of the divisor; NaN when it is zero. */
double mn_float_mod(double a, double b);

/* Float to Int. NaN gives 0; values out of range give the nearest Int. */
int64_t mn_float_to_i(double value); /* toward zero */
int64_t mn_float_floor(double value);
int64_t mn_float_ceil(double value);
int64_t mn_float_round(double value); /* halves away from zero */
/* `x.round(digits)` */
double mn_float_round_digits(double value, int64_t digits);

/* Conversions for stores into compact integer fields and elements: a
 * run-time error when the value does not fit. */
#define MN_DEFINE_NARROW(name, ctype, lo, hi, label)                          \
    static inline ctype name(int64_t value, MnLoc loc) {                       \
        if (value < (lo) || value > (hi)) mn_range_failed(value, label, loc);  \
        return (ctype)value;                                                   \
    }
MN_DEFINE_NARROW(mn_to_int8, int8_t, INT8_MIN, INT8_MAX, "Int8")
MN_DEFINE_NARROW(mn_to_int16, int16_t, INT16_MIN, INT16_MAX, "Int16")
MN_DEFINE_NARROW(mn_to_int32, int32_t, INT32_MIN, INT32_MAX, "Int32")
MN_DEFINE_NARROW(mn_to_uint8, uint8_t, 0, UINT8_MAX, "UInt8")
MN_DEFINE_NARROW(mn_to_uint16, uint16_t, 0, UINT16_MAX, "UInt16")
MN_DEFINE_NARROW(mn_to_uint32, uint32_t, 0, UINT32_MAX, "UInt32")
#undef MN_DEFINE_NARROW

/* ---- Random numbers, time ---------------------------------------------------- */

/* xoshiro256** seeded through splitmix64. The default seed is fixed, so a
 * program that never seeds produces the same numbers every run. */
double mn_rand(void);                      /* in [0, 1) */
int64_t mn_rand_int(int64_t n, MnLoc loc); /* in [0, n); n must be positive */
void mn_srand(int64_t seed);
void mn_srand_clock(void);

/* Real seconds since the program started. */
double mn_clock(void);
/* Flushes standard output, then pauses for that many real seconds;
 * returns at once for zero or less. */
void mn_sleep(double seconds);

/* ---- GPU systems ------------------------------------------------------------- */

/*
 * A GPU block is one `each` of a `gpu system`. Generated code describes
 * each block with a static MnGpuBlock and runs it with mn_gpu_run. The
 * runtime runs the block's shader when a device is in use and the block
 * fits it, and calls the block's C implementation otherwise. Both mean the
 * same thing:
 *
 * - The block runs once for every row of every table its outer query
 *   matches. A run reads and writes its own row of the outer columns.
 * - The nested query is read from a snapshot: for each inner column, the
 *   rows of every table the nested query matches, one table after another
 *   in the query's table order, as they were when mn_gpu_run was called.
 * - The uniform words are fixed when mn_gpu_run is called.
 * - A failing operation records its site and the run goes on. When every
 *   run has finished, mn_gpu_run reports the lowest site recorded, as a
 *   run-time error.
 *
 * Uniform words. A block's uniform data is `uniform_words` 32-bit words,
 * at least MN_GPU_HEADER_WORDS. The runtime fills the header and the
 * block's `fill` writes the rest:
 *
 *     word 0  u32  rows of the table being run
 *     word 1  u32  rows of the snapshot (0 without a nested query)
 *     word 2  u32  seed: the low 32 bits of the seed last given to srand,
 *                  exclusive-or the high 32 bits
 *     word 3  u32  the block's `id`
 *     word 4  i32  Time.tick, wrapped to 32 bits
 *     word 5  f32  Time.dt
 *     word 6  f32  Time.elapsed
 *     word 7       zero
 *
 * The shader. `wgsl` is one module with the compute entry point `main`,
 * `@workgroup_size(64)`. The run for row
 *
 *     global_invocation_id.x + global_invocation_id.y * MN_GPU_ROW_SPAN
 *
 * does nothing when that is not below word 0. Its bindings, all in group
 * 0, are numbered in this order:
 *
 *     the uniform words          var<uniform>, a struct of 32-bit scalars
 *     each outer column          var<storage, read_write> array<u32> when
 *                                `write`, var<storage, read> otherwise
 *     each inner column          var<storage, read> array<u32>
 *     the error word             var<storage, read_write> atomic<u32>, only
 *                                when `fallible`
 *
 * A column buffer holds the column's bytes exactly as CPU memory does,
 * padded with zeros to a whole number of words. The shader lowers the
 * error word to a site with atomicMin; it starts at UINT32_MAX.
 */

/* In place of a component id: the handles of the rows (MnEntity, 8 bytes). */
#define MN_GPU_ENTITIES UINT32_MAX
#define MN_GPU_HEADER_WORDS 8
#define MN_GPU_ROW_SPAN (65535u * 64u)

typedef struct MnGpuColumn {
    uint32_t component; /* a component id, or MN_GPU_ENTITIES */
    uint32_t size;      /* bytes per row; a multiple of 4 when `write` */
    bool write;         /* outer columns only */
} MnGpuColumn;

/* An operation that can fail. A program's sites are in source order, so
 * the lowest index is the earliest in the source. */
typedef struct MnGpuSite {
    MnLoc loc;
    const char *message; /* "division by zero" */
} MnGpuSite;

/* What the C implementation of a block is given for one table. */
typedef struct MnGpuRun {
    const uint32_t *uniform;
    /* One array per outer column, in order: the table's own. */
    void *const *outer;
    /* One array per inner column, in order: the snapshot. */
    const void *const *inner;
    /* Lower it to the site of a failing operation. */
    uint32_t *error;
} MnGpuRun;

typedef struct MnGpuBlock {
    const char *system; /* the system's name, for messages */
    uint32_t each;      /* which `each` of the system, from 1 */
    uint32_t id;        /* unique in the program */
    MnQuery *outer;
    const MnGpuColumn *outer_columns; /* no component twice */
    uint32_t outer_count;
    MnQuery *inner; /* NULL without a nested query */
    const MnGpuColumn *inner_columns;
    uint32_t inner_count;
    uint32_t uniform_words;
    /* Writes words MN_GPU_HEADER_WORDS and up. NULL when there are none. */
    void (*fill)(uint32_t *words);
    /* Runs every row of one table. */
    void (*cpu)(const MnGpuRun *run);
    const char *wgsl;
    bool fallible;
    /* The program's sites. NULL unless `fallible`. */
    const MnGpuSite *sites;
    /* Runtime-private. Initialise to NULL. */
    void *state;
} MnGpuBlock;

void mn_gpu_run(MnGpuBlock *block);

/* Whether any column has a GPU copy. */
extern bool mn_gpu_active;

void mn_gpu_sync(MnTable *table, uint32_t component, bool write);

/* Makes the CPU copy of a column current, and with `write` the only
 * current one. Ordinary generated code calls it before it uses mn_column
 * on a component that some GPU block names, once per table per loop. The
 * runtime does the same itself before a structural change to a table. */
static inline void mn_sync(MnTable *table, uint32_t component, bool write) {
    if (mn_gpu_active) mn_gpu_sync(table, component, write);
}

/* mn_get for a component that some GPU block names. */
static inline void *mn_get_synced(MnEntity e, uint32_t component, size_t size,
                                  bool write, MnLoc loc) {
    if (mn_gpu_active && mn_alive(e)) {
        uint32_t table = mn_world.slots[e.index].table;
        if (table != MN_NO_TABLE) {
            mn_gpu_sync(mn_world.tables[table], component, write);
        }
    }
    return mn_get(e, component, size, loc);
}

#endif
