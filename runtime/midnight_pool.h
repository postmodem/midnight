/*
 * The thread pool: runs the pieces of one loop on several threads. This
 * header is what midnight_rt.c and midnight_pool.c need from each other;
 * generated code never includes it. midnight_pool.c is compiled only into
 * programs built with MN_THREADS, and uses nothing of midnight_rt.c.
 *
 * - The pool runs one loop at a time, for one calling thread, which takes
 *   part in every loop as thread 0.
 * - The other threads are created at the first loop that more than one
 *   thread takes part in: one less than the thread count, each with every
 *   signal blocked and a stack as large as the calling thread's limit (8 MiB
 *   when that limit is unlimited).
 * - The pieces of a loop are numbered from 0. Each thread taking part has a
 *   region, an equal share of the pieces in order, and claims the pieces of
 *   its own region first, from the lowest up; a thread whose region is
 *   empty claims from the regions of the others. So thread `t` runs the
 *   same pieces whenever a loop has the same number of pieces and threads,
 *   as far as the other threads leave them to it, and any thread finishes
 *   any region.
 * - The threads that take part in a loop are the lowest-numbered ones. A
 *   thread with nothing to do polls for 50 microseconds and then blocks;
 *   the caller wakes the ones taking part that are blocked, and no others.
 */

#ifndef MIDNIGHT_POOL_H
#define MIDNIGHT_POOL_H

#include <stdbool.h>
#include <stdint.h>

/* The largest thread count. */
#define MN_POOL_MAX 256

typedef struct MnPoolOptions {
    /* The thread count, the calling thread among them: 1 to MN_POOL_MAX, or
     * 0 for the number of cores among the processors the process may run
     * on. */
    uint32_t threads;
    /* Every region is claimed from its last piece to its first, in a loop
     * that more than one thread takes part in. */
    bool reverse;
    /* Each thread yields at random, from a fixed seed, before each claim. */
    bool jitter;
    /* Threads never block: one with nothing to do keeps polling. */
    bool awake;
    /* The program writes JSON lines (MIDNIGHT_FORMAT=json): the pool's own
     * out-of-memory line is then {"kind":"out_of_memory"}. */
    bool json;
} MnPoolOptions;

/* Sets the options and the thread count. Call before the first loop, and
 * again only after mn_pool_finish. Creates no thread. */
void mn_pool_start(const MnPoolOptions *options);

/* The thread count: what mn_pool_start was given, or the number of cores.
 * Lower after a loop for which the system refused to create a thread. */
uint32_t mn_pool_threads(void);

typedef struct MnPoolLoop {
    /* Runs one piece. `thread` is below the number of threads taking part,
     * and no two calls at the same time have the same one. */
    void (*run)(void *context, uint64_t piece, uint32_t thread);
    /* NULL for a loop whose pieces cannot fail. Otherwise: called once, by
     * whichever thread finds it first, when some piece has failed and every
     * piece before the earliest failed one has finished; `piece` is that
     * earliest one. It must not return. */
    void (*failed)(void *context, uint64_t piece);
    void *context;
    uint64_t pieces;
    /* How many threads are to take part, the caller among them. The pool
     * lowers it to the thread count and to the number of pieces. */
    uint32_t threads;
} MnPoolLoop;

/* Runs every piece of the loop once and returns when all have finished.
 * With one thread taking part the pieces run in order on the caller,
 * whatever the options. Must not be called from inside a piece.
 *
 * When a piece fails, the loop does not return: no piece after the
 * earliest failed one is started from then on, pieces before it are still
 * run, and `failed` is called as soon as they have all finished, without
 * waiting for any piece after it that is still running. */
void mn_pool_loop(const MnPoolLoop *loop);

/* Called from `run`, by the thread that is running `piece`: the piece has
 * failed. The call returns, and `run` must then return without doing more
 * of the piece's work. Only in a loop with a `failed`. */
void mn_pool_fail(uint64_t piece);

/* Stops and joins the threads and releases what the pool holds. Must not
 * be called while a loop is running. */
void mn_pool_finish(void);

#endif
