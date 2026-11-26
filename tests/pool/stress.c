/*
 * Exercises runtime/midnight_pool.c on its own.
 *
 *     stress MODE [WORDS]
 *
 * WORDS is a comma-separated list of a thread count and the words
 * `reverse`, `jitter` and `awake`, as in MIDNIGHT_THREADS. The modes:
 *
 *     back      a million loops of a few pieces, one right after another
 *     idle      loops with pauses of 0.1 to 20 ms between them
 *     crowd     four threads for every processor
 *     one       the process confined to one processor (Linux)
 *     restart   the pool started and stopped a thousand times
 *     fail      a loop in which four pieces fail
 *     hang      a failing piece, and a later piece that never ends
 *     swarm     a piece for every thread, all of them short, two failing
 *
 * Every loop checks that each piece ran exactly once and that no two
 * pieces ran at the same time under one thread number. The first five
 * modes print `ok` and exit with status 0. `fail`, `hang` and `swarm`
 * print `failed N`, the earliest failing piece, and exit with status 3.
 */

#if defined(__linux__)
#define _GNU_SOURCE
#endif
#define _POSIX_C_SOURCE 200809L

#include "midnight_pool.h"

#include <sched.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

enum { MOST_PIECES = 20000 };

static _Atomic unsigned char ran[MOST_PIECES];

static struct {
    _Alignas(64) _Atomic int busy;
    uint64_t pieces;
} threads[MN_POOL_MAX];

static _Noreturn void die(const char *what, uint64_t number) {
    printf("stress: %s (%llu)\n", what, (unsigned long long)number);
    fflush(stdout);
    _Exit(1);
}

static void pause_ns(long ns) {
    struct timespec pause = {ns / 1000000000L, ns % 1000000000L};
    nanosleep(&pause, NULL);
}

/* Counts one run of a piece for the checks of `loop`. */
static void count_piece(uint64_t piece, uint32_t thread) {
    if (atomic_exchange(&threads[thread].busy, 1) != 0) {
        die("two pieces at once under one thread number", thread);
    }
    atomic_fetch_add(&ran[piece], 1);
    threads[thread].pieces++;
    atomic_store(&threads[thread].busy, 0);
}

static void plain_piece(void *context, uint64_t piece, uint32_t thread) {
    uint32_t taking = *(const uint32_t *)context;
    if (thread >= taking) die("a thread number too high", thread);
    count_piece(piece, thread);
}

/* Runs a loop of `pieces` pieces for `taking` threads and checks it. */
static void loop(uint64_t pieces, uint32_t taking) {
    if (pieces > MOST_PIECES) die("too many pieces", pieces);
    MnPoolLoop description = {plain_piece, NULL, &taking, pieces, taking};
    mn_pool_loop(&description);
    uint64_t counted = 0;
    for (uint32_t t = 0; t < MN_POOL_MAX; t++) {
        counted += threads[t].pieces;
        threads[t].pieces = 0;
    }
    if (counted != pieces) die("pieces run", counted);
    for (uint64_t piece = 0; piece < pieces; piece++) {
        if (atomic_exchange(&ran[piece], 0) != 1) die("a piece not run once", piece);
    }
}

/* The pieces of `fail` and `hang`. */

static const uint64_t failing[] = {700, 400, 401, 950};
static uint64_t endless = UINT64_MAX;

static void failing_piece(void *context, uint64_t piece, uint32_t thread) {
    (void)context;
    if (piece == endless) {
        for (;;) pause_ns(1000000);
    }
    /* Pieces before the failing ones take longer than those after, so
     * that a failure is found while earlier pieces still run. */
    if (piece < 400) pause_ns(20000);
    for (size_t i = 0; i < sizeof failing / sizeof failing[0]; i++) {
        if (piece == failing[i]) {
            mn_pool_fail(piece);
            return;
        }
    }
    count_piece(piece, thread);
}

/* The pieces of `swarm`: piece 5 and the last one fail, at once. */
static void swarm_piece(void *context, uint64_t piece, uint32_t thread) {
    uint64_t pieces = *(const uint64_t *)context;
    if (piece == 5 || piece == pieces - 1) {
        mn_pool_fail(piece);
        return;
    }
    count_piece(piece, thread);
}

static _Noreturn void report(void *context, uint64_t piece) {
    (void)context;
    for (uint64_t before = 0; before < piece; before++) {
        if (atomic_load(&ran[before]) != 1) die("a piece before the failure not run once", before);
    }
    printf("failed %llu\n", (unsigned long long)piece);
    fflush(stdout);
    _Exit(3);
}

static MnPoolOptions parse(const char *words) {
    MnPoolOptions options = {0};
    while (words && *words != '\0') {
        size_t len = strcspn(words, ",");
        if (len == 7 && memcmp(words, "reverse", len) == 0) {
            options.reverse = true;
        } else if (len == 6 && memcmp(words, "jitter", len) == 0) {
            options.jitter = true;
        } else if (len == 5 && memcmp(words, "awake", len) == 0) {
            options.awake = true;
        } else {
            options.threads = (uint32_t)strtoul(words, NULL, 10);
            if (options.threads < 1 || options.threads > MN_POOL_MAX) die("bad word", 0);
        }
        words += len;
        if (*words == ',') words++;
    }
    return options;
}

int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "usage: stress MODE [WORDS]\n");
        return 2;
    }
    const char *mode = argv[1];
    MnPoolOptions options = parse(argc > 2 ? argv[2] : NULL);
    uint32_t asked = options.threads ? options.threads : 4;

    if (strcmp(mode, "back") == 0) {
        options.threads = asked;
        mn_pool_start(&options);
        for (uint32_t i = 0; i < 1000000; i++) {
            loop(i % 1000 == 999 ? 10000 : 2 + i % 7, asked);
        }
        mn_pool_finish();
    } else if (strcmp(mode, "idle") == 0) {
        static const long pauses[] = {100000, 1000000, 5000000, 20000000};
        options.threads = asked;
        mn_pool_start(&options);
        for (uint32_t i = 0; i < 200; i++) {
            pause_ns(pauses[i % 4]);
            loop(1 + i % 64, 1 + i % asked);
            loop(3, asked);
        }
        mn_pool_finish();
    } else if (strcmp(mode, "crowd") == 0) {
        long processors = sysconf(_SC_NPROCESSORS_ONLN);
        uint32_t crowd = processors < 2 ? 8 : (uint32_t)processors * 4;
        if (crowd > MN_POOL_MAX) crowd = MN_POOL_MAX;
        options.threads = crowd;
        mn_pool_start(&options);
        for (uint32_t i = 0; i < 2000; i++) {
            loop(1 + (i * 37) % 5000, 1 + i % crowd);
        }
        mn_pool_finish();
    } else if (strcmp(mode, "one") == 0) {
#if defined(__linux__)
        cpu_set_t allowed;
        if (sched_getaffinity(0, sizeof allowed, &allowed) != 0) die("affinity", 0);
        int first = 0;
        while (!CPU_ISSET(first, &allowed)) first++;
        CPU_ZERO(&allowed);
        CPU_SET(first, &allowed);
        if (sched_setaffinity(0, sizeof allowed, &allowed) != 0) die("affinity", 1);
        /* One processor is one core: the pool chooses one thread. */
        MnPoolOptions unset = options;
        unset.threads = 0;
        mn_pool_start(&unset);
        if (mn_pool_threads() != 1) die("threads on one processor", mn_pool_threads());
        for (uint32_t i = 0; i < 1000; i++) loop(1 + i % 50, 4);
        mn_pool_finish();
#endif
        options.threads = asked;
        mn_pool_start(&options);
        for (uint32_t i = 0; i < 20000; i++) loop(2 + i % 7, asked);
        mn_pool_finish();
    } else if (strcmp(mode, "restart") == 0) {
        for (uint32_t i = 0; i < 1000; i++) {
            options.threads = 2 + i % asked;
            mn_pool_start(&options);
            if (i % 5 != 0) {
                loop(3 + i % 9, options.threads);
                loop(1000, options.threads);
                loop(2, 2);
            }
            mn_pool_finish();
        }
    } else if (strcmp(mode, "fail") == 0 || strcmp(mode, "hang") == 0) {
        options.threads = asked;
        mn_pool_start(&options);
        loop(100, asked);
        if (strcmp(mode, "hang") == 0) endless = 800;
        MnPoolLoop description = {failing_piece, report, NULL, 1000, asked};
        mn_pool_loop(&description);
        die("a failed loop returned", 0);
    } else if (strcmp(mode, "swarm") == 0) {
        /* Every thread is in the loop when the pieces fail, and every one
         * of them then waits for the pieces before the failure. */
        uint64_t pieces = options.threads ? options.threads : 64;
        options.threads = (uint32_t)pieces;
        mn_pool_start(&options);
        loop(pieces, (uint32_t)pieces);
        MnPoolLoop description = {swarm_piece, report, &pieces, pieces, (uint32_t)pieces};
        mn_pool_loop(&description);
        die("a failed loop returned", 0);
    } else {
        fprintf(stderr, "stress: unknown mode `%s`\n", mode);
        return 2;
    }
    puts("ok");
    return 0;
}
