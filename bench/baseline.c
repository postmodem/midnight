/*
 * The work of examples/bench.grem written by hand: one million positions
 * and velocities in two arrays, moved for 100 ticks by one counted loop.
 * Prints the same lines as bench.grem.
 */

#define _POSIX_C_SOURCE 200809L

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define ENTITIES 1000000
#define TICKS 100

typedef struct {
    float x, y;
} Position;

typedef struct {
    float dx, dy;
} Velocity;

/* Simulated seconds per tick. Not constant, so move() has to load it. */
extern double time_dt;
double time_dt = 1.0 / 60;

void move(Position *restrict pos, const Velocity *restrict vel, size_t count);

/*
 * One tick. Each field is widened to a double, the arithmetic is done at
 * 64 bits and the result is stored as a float, which is what Gremlin does
 * with Float32 fields. Not inlined, so the tick loop pays for a call per
 * tick as a Gremlin program does.
 */
__attribute__((noinline)) void move(Position *restrict pos,
                                    const Velocity *restrict vel,
                                    size_t count) {
    const double dt = time_dt;
    for (size_t i = 0; i < count; i++) {
        pos[i].x = (float)((double)pos[i].x + (double)vel[i].dx * dt);
        pos[i].y = (float)((double)pos[i].y + (double)vel[i].dy * dt);
    }
}

static double round_to(double value, int digits) {
    double scale = pow(10.0, digits);
    return round(value * scale) / scale;
}

/*
 * Prints "key: value" with the value written as Gremlin writes a Float:
 * the shortest text that reads back as the same number, always with a
 * fractional part.
 */
static void print_float(const char *key, double value) {
    char text[40];
    for (int digits = 1; digits <= 17; digits++) {
        snprintf(text, sizeof text, "%.*g", digits, value);
        if (strtod(text, NULL) == value) {
            break;
        }
    }
    const char *suffix = strpbrk(text, ".en") ? "" : ".0";
    printf("%s: %s%s\n", key, text, suffix);
}

int main(void) {
    Position *pos = malloc(ENTITIES * sizeof *pos);
    Velocity *vel = malloc(ENTITIES * sizeof *vel);
    if (pos == NULL || vel == NULL) {
        fputs("baseline: out of memory\n", stderr);
        return 1;
    }

    /* The same starting values as bench.grem gives entity i. */
    for (long i = 0; i < ENTITIES; i++) {
        pos[i].x = (float)(i % 1000);
        pos[i].y = (float)(i / 1000);
        vel[i].dx = (float)((double)(i % 7 - 3) * 0.5);
        vel[i].dy = (float)((double)(i % 5 - 2) * 0.25);
    }

    /* Time the ticks only. */
    struct timespec start, end;
    clock_gettime(CLOCK_MONOTONIC, &start);
    for (int tick = 0; tick < TICKS; tick++) {
        move(pos, vel, ENTITIES);
    }
    clock_gettime(CLOCK_MONOTONIC, &end);
    double seconds = (double)(end.tv_sec - start.tv_sec) +
                     (double)(end.tv_nsec - start.tv_nsec) / 1e9;

    /* The sum of every x, added in index order. */
    double checksum = 0.0;
    for (long i = 0; i < ENTITIES; i++) {
        checksum += (double)pos[i].x;
    }

    printf("entities: %d\n", ENTITIES);
    printf("ticks: %d\n", TICKS);
    print_float("seconds", round_to(seconds, 6));
    print_float("ms_per_tick", round_to(seconds * 1000 / TICKS, 3));
    printf("updates_per_second: %lld\n",
           llround((double)ENTITIES * TICKS / seconds));
    print_float("checksum", checksum);

    free(pos);
    free(vel);
    return 0;
}
