/*
 * The Gremlin runtime: everything a compiled program needs that does not
 * depend on the program's own types. Generated code includes this header
 * and is compiled together with gremlin_rt.c.
 *
 * Conventions
 *
 * - All-zero bytes are a valid value of every type: 0, 0.0, false, the
 *   empty string (NULL), the empty list (NULL), Entity.none and the first
 *   variant of an enum. The runtime zero-fills storage it hands out.
 * - Strings and lists are pointers to reference-counted buffers. A function
 *   that returns a GrString or GrList returns an owned reference (the caller
 *   must release it or store it). Arguments are borrowed unless a function
 *   says it takes ownership.
 * - Values handed to the world (spawn, add, emit) are moved in: the caller
 *   must not drop them afterwards.
 * - Counts are not atomic. Nothing here is thread-safe.
 * - A GrLoc names the .grem line an operation came from, for run-time
 *   errors.
 *
 * Run-time errors print
 *
 *     FILE:LINE: runtime error: MESSAGE
 *
 * to standard error (after flushing standard output) and exit with status
 * 1. The messages are:
 *
 *     Entity(7v1) has no Health                    gr_get, entity alive
 *     Entity(7v1) is not alive                     gr_get, entity dead
 *     index 5 out of range for a list of length 3  indexing, delete_at
 *     pop on an empty list                         also first, last
 *     division by zero                             Int /
 *     modulo by zero                               Int %
 *     300 does not fit in UInt8                    compact integer stores
 *     negative exponent -2                         Int **
 *     rand(0): the argument must be positive
 *     World.tick called while a tick is in progress      also World.run
 *
 * With GR_DEBUG defined, the runtime counts live string and list buffers.
 * gr_finish then tears the world down and, if any buffer is still alive,
 * prints
 *
 *     gremlin: 2 buffers still alive at exit
 *
 * to standard error and exits with status 70.
 */

#ifndef GREMLIN_RT_H
#define GREMLIN_RT_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

/* ---- Errors ---------------------------------------------------------- */

typedef struct GrLoc {
    const char *file;
    int32_t line;
} GrLoc;

/* Reports a run-time error (printf-style message) and exits with status 1. */
_Noreturn void gr_panic(GrLoc loc, const char *format, ...)
    __attribute__((format(printf, 2, 3), cold));

/* ---- Program description --------------------------------------------- */

/* One per component or tag, indexed by component id. */
typedef struct GrComponentInfo {
    const char *name;
    uint32_t size;  /* 0 for a tag */
    uint32_t align; /* 1 for a tag */
    /* Releases what a value owns; NULL when it owns nothing. */
    void (*drop)(void *value);
} GrComponentInfo;

/* One per event type, indexed by event id. */
typedef struct GrEventInfo {
    const char *name;
    uint32_t size; /* at least 1 */
    uint32_t align;
    void (*drop)(void *value); /* as for components */
} GrEventInfo;

typedef struct GrProgram {
    const GrComponentInfo *components;
    uint32_t component_count;
    const GrEventInfo *events;
    uint32_t event_count;
    /* In the order World.tick runs them. */
    void (*const *systems)(void);
    uint32_t system_count;
    /* Releases what constants and resources own. Called by gr_finish when
     * GR_DEBUG is defined, before the buffer check. */
    void (*drop_globals)(void);
} GrProgram;

/* Call once, first. The program description must outlive the program. */
void gr_start(const GrProgram *program);

/* Call when the top-level statements have finished. Flushes standard
 * output. With GR_DEBUG it also applies nothing further, destroys every
 * entity, event and queued change, calls drop_globals, and exits with
 * status 70 if any string or list buffer is still alive. */
void gr_finish(void);

/* The built-in `exit`: flushes standard output and exits with the low
 * eight bits of status. No teardown and no buffer check. */
_Noreturn void gr_exit(int64_t status);

/* ---- Entities, tables and the world ----------------------------------- */

/* A handle: 64 bits. All-zero is Entity.none. Generations start at 1, so
 * no living entity has generation 0. Compare handles field by field. */
typedef struct GrEntity {
    uint32_t index;
    uint32_t generation;
} GrEntity;

/* Table index of an entity that is alive but in no table yet: one whose
 * spawn is queued. */
#define GR_NO_TABLE UINT32_MAX

/* One per entity index: 12 bytes. */
typedef struct GrSlot {
    uint32_t generation;
    uint32_t table; /* index into gr_world.tables, or GR_NO_TABLE */
    uint32_t row;
} GrSlot;

/* Entities with one set of components. The runtime may add fields after
 * the ones listed here. */
typedef struct GrTable {
    /* Indexed by component id. For a component the table has: its array
     * of `count` values. For a tag the table has: a non-NULL pointer that
     * must not be dereferenced. NULL for anything the table lacks. */
    void **columns;
    /* The handle of each row. */
    GrEntity *entities;
    uint32_t count;
    uint32_t capacity;
    /* Runtime-private fields follow. */
    uint32_t id;
    uint32_t set_count;
    uint32_t *set;
    uint64_t hash;
    uint32_t *add_edges;
    uint32_t *remove_edges;
} GrTable;

/* The runtime may add fields after the ones listed here. */
typedef struct GrWorld {
    GrSlot *slots;
    uint32_t slot_count;
    /* Tables are never destroyed or reordered while the program runs, and
     * none is created while defer_depth is above zero. */
    GrTable **tables;
    uint32_t table_count;
    /* Nesting depth of `each`. */
    uint32_t defer_depth;
    /* Number of queued structural changes. */
    uint32_t deferred_count;
    const GrProgram *program;
    /* Runtime-private fields follow. */
    uint32_t slot_capacity;
    uint32_t table_capacity;
    uint32_t free_head;
    uint32_t free_count;
    int64_t live_entities;
    bool in_tick;
    bool stop_requested;
} GrWorld;

extern GrWorld gr_world;

/* `e.alive?` */
static inline bool gr_alive(GrEntity e) {
    return e.index < gr_world.slot_count &&
           gr_world.slots[e.index].generation == e.generation;
}

/* `e.has?(C)`: false for a dead handle and for a queued spawn. */
static inline bool gr_has(GrEntity e, uint32_t component) {
    if (!gr_alive(e)) return false;
    uint32_t table = gr_world.slots[e.index].table;
    return table != GR_NO_TABLE &&
           gr_world.tables[table]->columns[component] != NULL;
}

_Noreturn void gr_get_failed(GrEntity e, uint32_t component, GrLoc loc)
    __attribute__((cold));

/* `e[C]`: the address of the entity's component, whose size the caller
 * passes. A run-time error if the entity is dead or lacks the component.
 * The address is valid only until the next structural change, tick or call
 * into user code; do not keep it. Never called for a tag. */
static inline void *gr_get(GrEntity e, uint32_t component, size_t size,
                           GrLoc loc) {
    if (gr_alive(e)) {
        const GrSlot *slot = &gr_world.slots[e.index];
        if (slot->table != GR_NO_TABLE) {
            char *column = gr_world.tables[slot->table]->columns[component];
            if (column) return column + (size_t)slot->row * size;
        }
    }
    gr_get_failed(e, component, loc);
}

/* The array of a component in a table, or NULL. Ask again at the start of
 * each table's loop; do not keep the address across a structural change. */
static inline void *gr_column(const GrTable *table, uint32_t component) {
    return table->columns[component];
}

/* ---- Queries ----------------------------------------------------------- */

/* The tables one `each` or `World.count` matches. Generated code keeps one
 * static GrQuery per use, initialised with its component lists and zeros
 * elsewhere:
 *
 *     static const uint32_t q0_with[] = {0, 1};
 *     static const uint32_t q0_without[] = {2};
 *     static GrQuery q0 = {q0_with, 2, q0_without, 1, 0, 0, 0, 0};
 */
typedef struct GrQuery {
    /* Every component the entity must have: the terms, then `with:`. */
    const uint32_t *with;
    uint32_t with_count;
    const uint32_t *without;
    uint32_t without_count;
    /* Matching tables, in the order they were created. Filled by
     * gr_query_update. Tables with no rows are included; skip them. */
    GrTable **tables;
    uint32_t table_count;
    uint32_t table_capacity;
    /* How many of the world's tables have been looked at. */
    uint32_t tables_seen;
} GrQuery;

void gr_query_refresh(GrQuery *query);

/* Brings `tables` up to date. Call before each use of a query. */
static inline void gr_query_update(GrQuery *query) {
    if (query->tables_seen != gr_world.table_count) gr_query_refresh(query);
}

/* `World.count(…)`: updates the query and adds up its tables' row counts. */
int64_t gr_query_count(GrQuery *query);

/* `World.count`: entities that are in a table. */
int64_t gr_entity_count(void);

/* ---- Structural changes ------------------------------------------------- */

/* While gr_world.defer_depth is above zero, gr_spawn, gr_add, gr_remove and
 * gr_despawn queue their work (taking ownership of the values at once) and
 * the world does not change. gr_defer_leave applies the queue, in order,
 * when the depth returns to zero. Generated code calls gr_defer_enter
 * before the loops of an `each` and gr_defer_leave on every way out. */
void gr_flush(void);

static inline void gr_defer_enter(void) { gr_world.defer_depth++; }

static inline void gr_defer_leave(void) {
    if (--gr_world.defer_depth == 0 && gr_world.deferred_count != 0) gr_flush();
}

/* The component list of one `spawn` or `add` in the source. Generated code
 * keeps one static GrBundle per use, initialised with its list and zeros
 * elsewhere; the runtime caches what it likes in the other fields. No
 * component is listed twice. */
typedef struct GrBundle {
    const uint32_t *components;
    uint32_t count;
    /* Runtime-private. */
    uint32_t cached_from;
    uint32_t cached_to;
    bool cached;
} GrBundle;

/* `spawn`: values[i] points at the value for components[i] and is ignored
 * for a tag; `values` may be NULL when the bundle has no component with
 * data. The values are moved in. When deferred, the returned handle is
 * alive but in no table until the queue is applied. */
GrEntity gr_spawn(GrBundle *bundle, void *const *values);

/* `e.add(…)`: as gr_spawn. A component the entity already has is replaced
 * (the old value is dropped). If the entity is dead, the values are dropped
 * and nothing else happens. */
void gr_add(GrEntity e, GrBundle *bundle, void *const *values);

/* `e.remove(…)`: absent components and dead entities are ignored. */
void gr_remove(GrEntity e, const uint32_t *components, uint32_t count);

/* `e.despawn`: nothing happens if the entity is dead. */
void gr_despawn(GrEntity e);

/* ---- Systems, ticks and time ------------------------------------------- */

typedef struct GrTime {
    int64_t tick;   /* ticks completed */
    double dt;      /* starts at 1.0 / 60 */
    double elapsed; /* simulated seconds completed */
} GrTime;

extern GrTime gr_time;

/* `World.tick`: runs every system once, then adds one to gr_time.tick and
 * dt to gr_time.elapsed, then discards events emitted before the end of
 * the previous tick. A run-time error if a tick is in progress. */
void gr_tick(GrLoc loc);

/* `World.run`: clears the stop flag, then ticks until a tick has finished
 * during which gr_stop was called. */
void gr_run(GrLoc loc);

/* `World.stop` */
void gr_stop(void);

/* ---- Events -------------------------------------------------------------- */

/* Every event of a type has a sequence number, counting from 0 in the
 * order emitted. An `on` in the source keeps a static cursor, the sequence
 * number it has read up to, starting at 0:
 *
 *     uint64_t end = gr_event_end(EV);
 *     uint64_t seq = cursor;
 *     cursor = end;
 *     for (; seq < end; seq++) {
 *         void *event = gr_event_at(EV, seq);
 *         if (!event) continue;
 *         ...copy the value out, then run the body...
 *     }
 */

/* `emit`: appends a zeroed slot for one event and returns it. The caller
 * moves the value in at once. */
void *gr_emit(uint32_t event);

/* One past the sequence number of the newest event of this type. */
uint64_t gr_event_end(uint32_t event);

/* The event with this sequence number, or NULL if it has been discarded.
 * The address is valid until the next emit or tick: copy the value out
 * before running any user code, and ask again for the next one. */
void *gr_event_at(uint32_t event, uint64_t seq);

/* ---- Strings ------------------------------------------------------------- */

/* An immutable byte string. NULL is the empty string; no buffer of length
 * zero exists. `refs` below zero marks a static literal that is never
 * counted or freed. */
typedef struct GrStr {
    int64_t refs;
    int64_t len;
    char bytes[];
} GrStr;

typedef GrStr *GrString;

/* Defines a static literal `name`, usable as `(GrString)&name`:
 *
 *     GR_STR_LIT(s0, 13, "Hello, world!");
 *
 * `len` is the byte length, which the C literal's terminating NUL follows.
 * Do not define a literal of length zero; use NULL. */
#define GR_STR_LIT(name, length, text)                                         \
    static struct {                                                            \
        int64_t refs;                                                          \
        int64_t len;                                                           \
        char bytes[(length) + 1];                                              \
    } name = {-1, (length), text}

void gr_str_free(GrString s);

static inline void gr_str_retain(GrString s) {
    if (s && s->refs > 0) s->refs++;
}

static inline void gr_str_release(GrString s) {
    if (s && s->refs > 0 && --s->refs == 0) gr_str_free(s);
}

static inline int64_t gr_str_bytesize(GrString s) { return s ? s->len : 0; }

/* A new string holding `len` bytes copied from `bytes`. */
GrString gr_str_new(const char *bytes, int64_t len);

GrString gr_str_concat(GrString a, GrString b);
/* `s * n`: empty when n <= 0. */
GrString gr_str_repeat(GrString s, int64_t n);
bool gr_str_eq(GrString a, GrString b);
/* Byte-wise order: negative, zero or positive. */
int gr_str_cmp(GrString a, GrString b);
/* Number of Unicode code points. */
int64_t gr_str_length(GrString s);
/* ASCII letters only. */
GrString gr_str_upcase(GrString s);
GrString gr_str_downcase(GrString s);
bool gr_str_include(GrString s, GrString part);
bool gr_str_start_with(GrString s, GrString prefix);
bool gr_str_end_with(GrString s, GrString suffix);
/* Padded with spaces to at least `width` code points. */
GrString gr_str_ljust(GrString s, int64_t width);
GrString gr_str_rjust(GrString s, int64_t width);
/* The number at the start of the string, after optional whitespace, or
 * zero. gr_str_to_i reads an optional sign and decimal digits (underscores
 * between digits allowed) and wraps on overflow. gr_str_to_f reads what
 * strtod reads, except hexadecimal, infinity and NaN. */
int64_t gr_str_to_i(GrString s);
double gr_str_to_f(GrString s);

/* `to_s` of the built-in types. */
GrString gr_int_to_s(int64_t value);
GrString gr_float_to_s(double value);
GrString gr_bool_to_s(bool value);
GrString gr_entity_to_s(GrEntity e);

/* ---- String builder ----------------------------------------------------- */

/* Accumulates text for interpolation, `to_s` and output. Zero-initialise
 * it; then append; then end with exactly one of gr_sb_finish or
 * gr_sb_write. Its storage is not a counted buffer.
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
typedef struct GrBuilder {
    char *bytes;
    int64_t len;
    int64_t capacity;
} GrBuilder;

void gr_sb_bytes(GrBuilder *sb, const char *bytes, int64_t len);
void gr_sb_str(GrBuilder *sb, GrString s);
void gr_sb_quoted(GrBuilder *sb, GrString s);
void gr_sb_int(GrBuilder *sb, int64_t value);
void gr_sb_float(GrBuilder *sb, double value);
void gr_sb_bool(GrBuilder *sb, bool value);
void gr_sb_entity(GrBuilder *sb, GrEntity e);

/* Returns the text as a new string and frees the builder's storage. */
GrString gr_sb_finish(GrBuilder *sb);
/* Writes the text to standard output and frees the builder's storage. */
void gr_sb_write(GrBuilder *sb);

/* ---- Output -------------------------------------------------------------- */

/* Standard output is buffered; the runtime flushes it before a run-time
 * error and at exit. */
void gr_out_bytes(const char *bytes, int64_t len);
void gr_out_str(GrString s);
void gr_out_int(int64_t value);
void gr_out_float(double value);
void gr_out_bool(bool value);
void gr_out_entity(GrEntity e);
void gr_out_newline(void);

/* ---- Lists ---------------------------------------------------------------- */

/* A list is a pointer to a counted buffer: this header, then `cap`
 * elements of which `len` are in use. NULL is the empty list. Elements
 * are aligned to at most 8 bytes. A buffer whose count is above one is
 * shared and must not be written; gr_list_unique makes a private copy. */
typedef struct GrListBuf {
    int64_t refs;
    int64_t len;
    int64_t cap;
} GrListBuf;

typedef GrListBuf *GrList;

/* How to handle one element type. Generated code keeps one static const
 * GrElem per list type. */
typedef struct GrElem {
    uint32_t size;
    /* Adds a reference to everything the element at `value` owns, after
     * its bytes have been copied. NULL when it owns nothing. */
    void (*retain)(void *value);
    /* Releases what the element owns. NULL when it owns nothing. */
    void (*drop)(void *value);
} GrElem;

static inline int64_t gr_list_len(GrList list) { return list ? list->len : 0; }

/* The first element. Only for a non-NULL list. */
static inline void *gr_list_data(GrList list) { return (void *)(list + 1); }

void gr_list_free(GrList list, const GrElem *elem);

static inline void gr_list_retain(GrList list) {
    if (list) list->refs++;
}

/* Drops the elements and frees the buffer when the last reference goes. */
static inline void gr_list_release(GrList list, const GrElem *elem) {
    if (list && --list->refs == 0) gr_list_free(list, elem);
}

/* A new list of `len` zeroed elements; NULL when len <= 0. The caller
 * fills the elements in. */
GrList gr_list_alloc(const GrElem *elem, int64_t len);

/* Makes `*list` safe to write: if its buffer is shared, replaces it with a
 * private copy (retaining each element) and releases the shared one. */
void gr_list_unique(GrList *list, const GrElem *elem);

_Noreturn void gr_index_failed(int64_t index, int64_t len, GrLoc loc)
    __attribute__((cold));

/* Checks an index and returns it counted from the start. A negative index
 * counts from the end. */
static inline int64_t gr_list_index(GrList list, int64_t index, GrLoc loc) {
    int64_t len = gr_list_len(list);
    int64_t i = index < 0 ? index + len : index;
    if (i < 0 || i >= len) gr_index_failed(index, len, loc);
    return i;
}

/* For `first`, `last` and `pop`: a run-time error naming `what` if the
 * list is empty, otherwise its length. */
int64_t gr_list_need(GrList list, const char *what, GrLoc loc);

/* `push`: makes the list unique, grows it by doubling when full, and
 * returns the new last element, zeroed, for the caller to move a value
 * into. */
void *gr_list_push(GrList *list, const GrElem *elem);

/* `pop`: moves the last element into `out` (the caller owns it). A
 * run-time error on an empty list. A list that becomes empty may keep its
 * buffer. */
void gr_list_pop(GrList *list, const GrElem *elem, void *out, GrLoc loc);

/* `delete_at`: moves the element at `index` (checked as in gr_list_index)
 * into `out` and closes the gap. */
void gr_list_delete_at(GrList *list, const GrElem *elem, int64_t index,
                       void *out, GrLoc loc);

/* `clear`: releases the buffer and sets `*list` to NULL. */
void gr_list_clear(GrList *list, const GrElem *elem);

/* `a + b` and `a * n`: a new list (or a retained operand when the result
 * would equal it). Elements are retained. */
GrList gr_list_concat(GrList a, GrList b, const GrElem *elem);
GrList gr_list_repeat(GrList list, int64_t n, const GrElem *elem);

/* ---- Numbers ---------------------------------------------------------------- */

_Noreturn void gr_div_failed(const char *what, GrLoc loc) __attribute__((cold));
_Noreturn void gr_range_failed(int64_t value, const char *type, GrLoc loc)
    __attribute__((cold));

/* Int `/`: rounds toward negative infinity; wraps for INT64_MIN / -1. */
static inline int64_t gr_int_div(int64_t a, int64_t b, GrLoc loc) {
    if (b == 0) gr_div_failed("division", loc);
    if (b == -1) return (int64_t)(0 - (uint64_t)a);
    int64_t q = a / b;
    return (a % b != 0 && ((a < 0) != (b < 0))) ? q - 1 : q;
}

/* Int `%`: the result has the sign of the divisor. */
static inline int64_t gr_int_mod(int64_t a, int64_t b, GrLoc loc) {
    if (b == 0) gr_div_failed("modulo", loc);
    if (b == -1) return 0;
    int64_t r = a % b;
    return (r != 0 && ((r < 0) != (b < 0))) ? r + b : r;
}

/* Int `**`: wraps on overflow. A run-time error for a negative exponent. */
int64_t gr_int_pow(int64_t base, int64_t exponent, GrLoc loc);

/* Float `%`: the result has the sign of the divisor; NaN when it is zero. */
double gr_float_mod(double a, double b);

/* Float to Int. NaN gives 0; values out of range give the nearest Int. */
int64_t gr_float_to_i(double value); /* toward zero */
int64_t gr_float_floor(double value);
int64_t gr_float_ceil(double value);
int64_t gr_float_round(double value); /* halves away from zero */
/* `x.round(digits)` */
double gr_float_round_digits(double value, int64_t digits);

/* Conversions for stores into compact integer fields and elements: a
 * run-time error when the value does not fit. */
#define GR_DEFINE_NARROW(name, ctype, lo, hi, label)                          \
    static inline ctype name(int64_t value, GrLoc loc) {                       \
        if (value < (lo) || value > (hi)) gr_range_failed(value, label, loc);  \
        return (ctype)value;                                                   \
    }
GR_DEFINE_NARROW(gr_to_int8, int8_t, INT8_MIN, INT8_MAX, "Int8")
GR_DEFINE_NARROW(gr_to_int16, int16_t, INT16_MIN, INT16_MAX, "Int16")
GR_DEFINE_NARROW(gr_to_int32, int32_t, INT32_MIN, INT32_MAX, "Int32")
GR_DEFINE_NARROW(gr_to_uint8, uint8_t, 0, UINT8_MAX, "UInt8")
GR_DEFINE_NARROW(gr_to_uint16, uint16_t, 0, UINT16_MAX, "UInt16")
GR_DEFINE_NARROW(gr_to_uint32, uint32_t, 0, UINT32_MAX, "UInt32")
#undef GR_DEFINE_NARROW

/* ---- Random numbers, time ---------------------------------------------------- */

/* xoshiro256** seeded through splitmix64. The default seed is fixed, so a
 * program that never seeds produces the same numbers every run. */
double gr_rand(void);                      /* in [0, 1) */
int64_t gr_rand_int(int64_t n, GrLoc loc); /* in [0, n); n must be positive */
void gr_srand(int64_t seed);
void gr_srand_clock(void);

/* Real seconds since the program started. */
double gr_clock(void);
/* Pauses for that many real seconds; returns at once for zero or less. */
void gr_sleep(double seconds);

#endif
