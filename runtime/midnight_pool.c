/*
 * The thread pool. See midnight_pool.h for what each function promises.
 */

#if defined(__linux__)
#define _GNU_SOURCE /* sched_getaffinity, syscall */
#elif defined(__APPLE__)
#define _DARWIN_C_SOURCE /* sysctlbyname */
#endif
#define _POSIX_C_SOURCE 200809L

#include "midnight_pool.h"

#include <limits.h>
#include <pthread.h>
#include <sched.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <time.h>
#include <unistd.h>

#if defined(__APPLE__)
#include <sys/sysctl.h>
#endif

/* ThreadSanitizer does not follow a futex, so a build with it blocks on a
 * mutex and a condition variable, as a system without futexes does. */
#if defined(__SANITIZE_THREAD__)
#define MN_POOL_TSAN 1
#elif defined(__has_feature)
#if __has_feature(thread_sanitizer)
#define MN_POOL_TSAN 1
#endif
#endif

#if defined(__linux__) && !defined(MN_POOL_TSAN)
#define MN_POOL_FUTEX 1
#include <linux/futex.h>
#include <sys/syscall.h>
#endif

#if defined(__x86_64__) || defined(__i386__)
#define cpu_relax() __builtin_ia32_pause()
#elif defined(__aarch64__)
#define cpu_relax() __asm__ volatile("isb")
#else
#define cpu_relax() ((void)0)
#endif

/* How long a thread with nothing to do polls before it blocks. */
#define POLL_NS UINT64_C(50000)
/* How long a thread waits between two looks at a loop in which a piece has
 * failed. */
#define FAIL_WAIT_NS 20000L
/* The stack of a thread when the calling thread's limit is unlimited. */
#define DEFAULT_STACK ((size_t)8 << 20)

/* What Worker.running holds when the thread runs no piece, and while it
 * is claiming one. */
#define IDLE UINT64_MAX
#define CLAIMING (UINT64_MAX - 1)

/* Pool.inside while no thread may enter the loop. */
#define CLOSED UINT32_C(0x80000000)

/* One thread, and the region of the loop that is its own. Entry 0 is the
 * calling thread. */
typedef struct Worker {
    /* How many pieces of the region have been claimed. Claimed by any
     * thread. */
    _Alignas(64) _Atomic uint64_t next;
    /* The region: pieces `begin` up to `end`. Set before the loop opens. */
    uint64_t begin;
    uint64_t end;
    /* The piece this thread is running, IDLE or CLAIMING. Written by this
     * thread only, and only in a loop whose pieces can fail. */
    _Alignas(64) _Atomic uint64_t running;
    /* 1 while the thread is blocked or about to block. */
    _Atomic uint32_t asleep;
    uint32_t index;
    /* The loop count the thread has seen. Its own, once it runs. */
    uint32_t seen;
    /* The state of the `jitter` generator. */
    uint64_t random;
    pthread_t handle;
#ifndef MN_POOL_FUTEX
    pthread_cond_t wake;
#endif
} Worker;

static struct {
    bool started;
    MnPoolOptions options;
    /* The thread count. */
    uint32_t threads;
    /* Whether the threads have been asked for, and how many exist besides
     * the caller: workers[1] up to workers[created]. */
    bool spawned;
    uint32_t created;
    Worker *workers;
#ifndef MN_POOL_FUTEX
    /* Guards the loop count, `asleep` and `caller_parked` wherever a
     * thread blocks on them. */
    pthread_mutex_t mutex;
    /* Signalled when the last thread leaves a loop the caller waits for. */
    pthread_cond_t done;
#endif
    /* Held by the thread that reports a failed loop. */
    pthread_mutex_t fail_mutex;

    /* The loop being run and the number of threads taking part. Written
     * by the caller while the loop is closed. */
    MnPoolLoop loop;
    uint32_t taking;
    /* Regions are claimed from their last piece: the `reverse` option, in
     * a loop that more than one thread takes part in. */
    bool reverse;

    /* Counts the loops. A thread with nothing to do waits for it to
     * change. */
    _Alignas(64) _Atomic uint32_t epoch;
    _Atomic uint32_t shutdown;
    /* The threads inside the loop besides the caller, or CLOSED. */
    _Alignas(64) _Atomic uint32_t inside;
    /* 1 while the caller is blocked, or about to block, until `inside` is
     * zero. */
    _Atomic uint32_t caller_parked;
    /* No piece numbered this or above is started: the number of pieces,
     * or the earliest piece that has failed. */
    _Alignas(64) _Atomic uint64_t limit;
} pool = {
#ifndef MN_POOL_FUTEX
    .mutex = PTHREAD_MUTEX_INITIALIZER,
    .done = PTHREAD_COND_INITIALIZER,
#endif
    .fail_mutex = PTHREAD_MUTEX_INITIALIZER,
};

static _Noreturn void pool_abort(const char *what) {
    fflush(stdout);
    fprintf(stderr, "midnight: %s\n", what);
    abort();
}

static uint64_t now_ns(void) {
    struct timespec now = {0, 0};
    clock_gettime(CLOCK_MONOTONIC, &now);
    return (uint64_t)now.tv_sec * UINT64_C(1000000000) + (uint64_t)now.tv_nsec;
}

#ifdef MN_POOL_FUTEX
/* Blocks while `*word` is `expected`, until futex_wake. May return early. */
static void futex_wait(_Atomic uint32_t *word, uint32_t expected) {
    syscall(SYS_futex, (uint32_t *)word, FUTEX_WAIT_PRIVATE, expected, NULL,
            NULL, 0);
}

static void futex_wake(_Atomic uint32_t *word) {
    syscall(SYS_futex, (uint32_t *)word, FUTEX_WAKE_PRIVATE, 1, NULL, NULL, 0);
}
#endif

/* ---- The thread count ---------------------------------------------------- */

#if defined(__linux__)
/* A number from a file of /sys/devices/system/cpu, or -1. */
static long topology(int cpu, const char *name) {
    char path[96];
    snprintf(path, sizeof path, "/sys/devices/system/cpu/cpu%d/topology/%s",
             cpu, name);
    FILE *file = fopen(path, "r");
    if (!file) return -1;
    long value = -1;
    if (fscanf(file, "%ld", &value) != 1) value = -1;
    fclose(file);
    return value;
}
#endif

/* The cores among the processors the process may run on, or those
 * processors where the cores can't be told apart. At least 1. */
static uint32_t core_count(void) {
    long processors = -1;
#if defined(__linux__)
    cpu_set_t allowed;
    if (sched_getaffinity(0, sizeof allowed, &allowed) == 0) {
        static struct {
            long package;
            long core;
        } seen[CPU_SETSIZE];
        uint32_t cores = 0;
        bool known = true;
        processors = 0;
        for (int cpu = 0; cpu < CPU_SETSIZE; cpu++) {
            if (!CPU_ISSET(cpu, &allowed)) continue;
            processors++;
            if (!known) continue;
            long package = topology(cpu, "physical_package_id");
            long core = topology(cpu, "core_id");
            if (package < 0 || core < 0) {
                known = false;
                continue;
            }
            uint32_t at = 0;
            while (at < cores &&
                   (seen[at].package != package || seen[at].core != core)) {
                at++;
            }
            if (at == cores) {
                seen[cores].package = package;
                seen[cores].core = core;
                cores++;
            }
        }
        if (known && cores != 0) processors = (long)cores;
    }
#elif defined(__APPLE__)
    int count = 0;
    size_t size = sizeof count;
    if (sysctlbyname("hw.perflevel0.physicalcpu", &count, &size, NULL, 0) == 0 &&
        count > 0) {
        processors = count;
    } else {
        size = sizeof count;
        if (sysctlbyname("hw.physicalcpu", &count, &size, NULL, 0) == 0 &&
            count > 0) {
            processors = count;
        }
    }
#endif
    if (processors < 1) processors = sysconf(_SC_NPROCESSORS_ONLN);
    if (processors < 1) processors = 1;
    if (processors > MN_POOL_MAX) processors = MN_POOL_MAX;
    return (uint32_t)processors;
}

void mn_pool_start(const MnPoolOptions *options) {
    pool.started = true;
    pool.options = *options;
    uint32_t threads = options->threads;
    if (threads == 0) threads = core_count();
    if (threads > MN_POOL_MAX) threads = MN_POOL_MAX;
    pool.threads = threads;
    pool.spawned = false;
    pool.created = 0;
    void *memory = NULL;
    if (posix_memalign(&memory, _Alignof(Worker), threads * sizeof(Worker)) != 0) {
        pool_abort("out of memory");
    }
    pool.workers = memory;
    memset(pool.workers, 0, threads * sizeof(Worker));
    for (uint32_t t = 0; t < threads; t++) {
        Worker *worker = &pool.workers[t];
        worker->index = t;
        worker->random = UINT64_C(0x9e3779b97f4a7c15) * (t + 1);
        atomic_store(&worker->running, IDLE);
    }
    atomic_store(&pool.shutdown, 0);
    atomic_store(&pool.inside, CLOSED);
    atomic_store(&pool.caller_parked, 0);
}

static void ensure_started(void) {
    if (!pool.started) mn_pool_start(&(MnPoolOptions){0});
}

uint32_t mn_pool_threads(void) {
    ensure_started();
    return pool.threads;
}

/* ---- Claiming pieces ------------------------------------------------------ */

/* With `jitter`: gives the processor away before some claims, and sleeps
 * for up to 16 microseconds before a few. */
static void jitter(Worker *me) {
    uint64_t x = me->random;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    me->random = x;
    if ((x & 3) == 0) sched_yield();
    if (((x >> 8) & 63) == 0) {
        struct timespec pause = {0, (long)((x >> 16) & 15) * 1000 + 1};
        nanosleep(&pause, NULL);
    }
}

/* Claims a piece below the limit from a region. False when the region has
 * none left. */
static bool region_claim(Worker *region, uint64_t *piece) {
    uint64_t size = region->end - region->begin;
    for (;;) {
        if (atomic_load_explicit(&region->next, memory_order_relaxed) >= size) {
            return false;
        }
        uint64_t taken = atomic_fetch_add(&region->next, 1);
        if (taken >= size) return false;
        bool reverse = pool.reverse;
        uint64_t claimed =
            reverse ? region->end - 1 - taken : region->begin + taken;
        if (claimed < atomic_load(&pool.limit)) {
            *piece = claimed;
            return true;
        }
        /* Counting up, the rest of the region is past the limit too. */
        if (!reverse) return false;
    }
}

/* Runs pieces until no region has one below the limit: the thread's own
 * region first, then the others in order. */
static void claim_all(Worker *me) {
    const MnPoolLoop *loop = &pool.loop;
    uint32_t taking = pool.taking;
    bool can_fail = loop->failed != NULL;
    for (uint32_t step = 0; step < taking; step++) {
        Worker *region = &pool.workers[(me->index + step) % taking];
        for (;;) {
            if (pool.options.jitter) jitter(me);
            /* The order matters to all_before_finished: a thread says it
             * is claiming before it takes a piece off a region. */
            if (can_fail) {
                atomic_store_explicit(&me->running, CLAIMING,
                                      memory_order_relaxed);
            }
            uint64_t piece;
            if (!region_claim(region, &piece)) break;
            if (can_fail) {
                atomic_store_explicit(&me->running, piece,
                                      memory_order_relaxed);
            }
            loop->run(loop->context, piece, me->index);
        }
    }
    if (can_fail) atomic_store(&me->running, IDLE);
}

/* Whether every piece below `limit` has been claimed and none of them is
 * still running. */
static bool all_before_finished(uint64_t limit) {
    for (uint32_t t = 0; t < pool.taking; t++) {
        const Worker *region = &pool.workers[t];
        uint64_t need = 0;
        if (limit > region->begin) {
            uint64_t end = limit < region->end ? limit : region->end;
            need = pool.reverse ? region->end - region->begin
                                        : end - region->begin;
        }
        if (atomic_load(&region->next) < need) return false;
    }
    /* A thread that took one of those pieces off its region did so before
     * the counts above were read, so what it is running shows here. */
    for (uint32_t t = 0; t < pool.taking; t++) {
        uint64_t running = atomic_load(&pool.workers[t].running);
        if (running == CLAIMING || running < limit) return false;
    }
    return true;
}

/* A piece has failed, and this thread has been through every region: each
 * piece before the earliest failed one has been claimed, by this thread or
 * another, and a later failure leaves no more of them to claim. Waits for
 * the ones that are still running, and reports. The thread claims nothing
 * while it waits, so that it does not look to the others as if it might
 * be about to run one. */
static _Noreturn void fail_wait(void) {
    const MnPoolLoop *loop = &pool.loop;
    for (;;) {
        pthread_mutex_lock(&pool.fail_mutex);
        uint64_t limit = atomic_load(&pool.limit);
        if (all_before_finished(limit)) loop->failed(loop->context, limit);
        pthread_mutex_unlock(&pool.fail_mutex);
        struct timespec pause = {0, FAIL_WAIT_NS};
        nanosleep(&pause, NULL);
    }
}

/* What a thread does in a loop it takes part in. */
static void run_pieces(Worker *me) {
    claim_all(me);
    if (pool.loop.failed &&
        atomic_load(&pool.limit) != pool.loop.pieces) {
        fail_wait();
    }
}

void mn_pool_fail(uint64_t piece) {
    uint64_t limit = atomic_load(&pool.limit);
    while (piece < limit &&
           !atomic_compare_exchange_weak(&pool.limit, &limit, piece)) {
    }
}

/* ---- The threads ---------------------------------------------------------- */

/* Counts a thread into the loop. False when the loop is closed. */
static bool loop_enter(void) {
    uint32_t inside = atomic_load(&pool.inside);
    for (;;) {
        if (inside & CLOSED) return false;
        if (atomic_compare_exchange_weak(&pool.inside, &inside, inside + 1)) {
            return true;
        }
    }
}

static void loop_leave(void) {
    uint32_t left = atomic_fetch_sub(&pool.inside, 1) - 1;
    if (left != 0 || atomic_load(&pool.caller_parked) == 0) return;
#ifdef MN_POOL_FUTEX
    futex_wake(&pool.inside);
#else
    pthread_mutex_lock(&pool.mutex);
    pthread_cond_signal(&pool.done);
    pthread_mutex_unlock(&pool.mutex);
#endif
}

/* Returns the loop count once it is no longer the one the thread has
 * seen: polls, then blocks unless `awake`. */
static uint32_t next_epoch(Worker *me) {
    uint32_t seen = me->seen;
    for (;;) {
        uint64_t started = now_ns();
        do {
            for (int i = 0; i < 16; i++) {
                uint32_t epoch = atomic_load(&pool.epoch);
                if (epoch != seen) return epoch;
                cpu_relax();
            }
        } while (pool.options.awake || now_ns() - started < POLL_NS);
#ifdef MN_POOL_FUTEX
        atomic_store(&me->asleep, 1);
        if (atomic_load(&pool.epoch) == seen) futex_wait(&me->asleep, 1);
        atomic_store(&me->asleep, 0);
#else
        pthread_mutex_lock(&pool.mutex);
        while (atomic_load(&pool.epoch) == seen) {
            atomic_store(&me->asleep, 1);
            pthread_cond_wait(&me->wake, &pool.mutex);
        }
        atomic_store(&me->asleep, 0);
        pthread_mutex_unlock(&pool.mutex);
#endif
    }
}

static void *worker_main(void *arg) {
    Worker *me = arg;
    for (;;) {
        me->seen = next_epoch(me);
        if (atomic_load(&pool.shutdown)) break;
        if (!loop_enter()) continue;
        if (me->index < pool.taking) run_pieces(me);
        loop_leave();
    }
    return NULL;
}

/* Starts a new loop count and wakes the blocked ones among workers 1 up
 * to `count` - 1. */
static void wake_workers(uint32_t count) {
#ifdef MN_POOL_FUTEX
    atomic_fetch_add(&pool.epoch, 1);
    for (uint32_t t = 1; t < count; t++) {
        Worker *worker = &pool.workers[t];
        if (atomic_load(&worker->asleep) != 0 &&
            atomic_exchange(&worker->asleep, 0) != 0) {
            futex_wake(&worker->asleep);
        }
    }
#else
    pthread_mutex_lock(&pool.mutex);
    atomic_fetch_add(&pool.epoch, 1);
    for (uint32_t t = 1; t < count; t++) {
        Worker *worker = &pool.workers[t];
        if (atomic_load(&worker->asleep) != 0) {
            pthread_cond_signal(&worker->wake);
        }
    }
    pthread_mutex_unlock(&pool.mutex);
#endif
}

/* Creates the threads: every signal blocked, and a stack as large as this
 * thread's limit. Lowers the thread count to what the system allows. */
static void threads_create(void) {
    pool.spawned = true;
    sigset_t every, before;
    sigfillset(&every);
    pthread_sigmask(SIG_SETMASK, &every, &before);

    size_t stack = DEFAULT_STACK;
    struct rlimit limit;
    if (getrlimit(RLIMIT_STACK, &limit) == 0 &&
        limit.rlim_cur != RLIM_INFINITY &&
        limit.rlim_cur >= (rlim_t)PTHREAD_STACK_MIN &&
        limit.rlim_cur <= (rlim_t)(SIZE_MAX / 2)) {
        stack = (size_t)limit.rlim_cur;
    }
    long page = sysconf(_SC_PAGESIZE);
    if (page > 0) stack = (stack + (size_t)page - 1) / (size_t)page * (size_t)page;
    pthread_attr_t sized;
    pthread_attr_init(&sized);
    bool has_size = pthread_attr_setstacksize(&sized, stack) == 0;

    uint32_t epoch = atomic_load(&pool.epoch);
    for (uint32_t t = 1; t < pool.threads; t++) {
        Worker *worker = &pool.workers[t];
        worker->seen = epoch;
#ifndef MN_POOL_FUTEX
        pthread_cond_init(&worker->wake, NULL);
#endif
        int refused = has_size ? pthread_create(&worker->handle, &sized,
                                                worker_main, worker)
                               : 1;
        if (refused) {
            refused = pthread_create(&worker->handle, NULL, worker_main, worker);
        }
        if (refused) {
#ifndef MN_POOL_FUTEX
            pthread_cond_destroy(&worker->wake);
#endif
            break;
        }
        pool.created = t;
    }
    pool.threads = pool.created + 1;
    pthread_attr_destroy(&sized);
    pthread_sigmask(SIG_SETMASK, &before, NULL);
}

/* Waits until no other thread is inside the loop, and closes it. */
static void loop_close(void) {
    uint64_t started = now_ns();
    for (;;) {
        for (int i = 0; i < 16; i++) {
            uint32_t none = 0;
            if (atomic_compare_exchange_weak(&pool.inside, &none, CLOSED)) {
                return;
            }
            cpu_relax();
        }
        if (now_ns() - started < POLL_NS) continue;
#ifdef MN_POOL_FUTEX
        atomic_store(&pool.caller_parked, 1);
        uint32_t busy = atomic_load(&pool.inside);
        if (busy != 0) futex_wait(&pool.inside, busy);
        atomic_store(&pool.caller_parked, 0);
#else
        pthread_mutex_lock(&pool.mutex);
        atomic_store(&pool.caller_parked, 1);
        while (atomic_load(&pool.inside) != 0) {
            pthread_cond_wait(&pool.done, &pool.mutex);
        }
        atomic_store(&pool.caller_parked, 0);
        pthread_mutex_unlock(&pool.mutex);
#endif
    }
}

void mn_pool_loop(const MnPoolLoop *loop) {
    ensure_started();
    uint64_t pieces = loop->pieces;
    if (pieces == 0) return;
    uint32_t taking = loop->threads;
    if (taking > pool.threads) taking = pool.threads;
    if (taking > pieces) taking = (uint32_t)pieces;
    if (taking > 1 && !pool.spawned) {
        threads_create();
        if (taking > pool.threads) taking = pool.threads;
    }
    if (taking < 1) taking = 1;

    pool.loop = *loop;
    pool.taking = taking;
    pool.reverse = pool.options.reverse && taking > 1;
    atomic_store(&pool.limit, pieces);
    uint64_t share = pieces / taking;
    uint64_t extra = pieces % taking;
    uint64_t begin = 0;
    for (uint32_t t = 0; t < taking; t++) {
        Worker *region = &pool.workers[t];
        region->begin = begin;
        begin += share + (t < extra ? 1 : 0);
        region->end = begin;
        atomic_store(&region->next, 0);
        atomic_store(&region->running, IDLE);
    }
    if (taking == 1) {
        run_pieces(&pool.workers[0]);
        return;
    }
    atomic_store(&pool.inside, 0);
    wake_workers(taking);
    run_pieces(&pool.workers[0]);
    loop_close();
}

void mn_pool_finish(void) {
    if (!pool.started) return;
    if (pool.created != 0) {
        atomic_store(&pool.shutdown, 1);
        wake_workers(pool.created + 1);
        for (uint32_t t = 1; t <= pool.created; t++) {
            pthread_join(pool.workers[t].handle, NULL);
#ifndef MN_POOL_FUTEX
            pthread_cond_destroy(&pool.workers[t].wake);
#endif
        }
    }
    free(pool.workers);
    pool.workers = NULL;
    pool.threads = 0;
    pool.created = 0;
    pool.spawned = false;
    pool.started = false;
}
