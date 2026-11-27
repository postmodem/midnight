/*
 * The runtime with a main of its own, for what no Midnight program can
 * make it do on demand: the text of every 32-bit float, and the lines of
 * a GPU error, a window error, running out of memory and buffers alive at
 * exit.
 *
 *   lines float32 all|sample THREADS
 *       Checks the Float32 rule: each text reads back as the same value
 *       through strtof and through strtod rounded to 32 bits, and no text
 *       with fewer significant digits does both. `all` checks every finite
 *       pattern, `sample` every power of two, ±7.0385307e-26 and a fixed
 *       sample of a million patterns. Prints the number checked.
 *   lines gpu_error | window_error | out_of_memory | buffers_alive
 *       Writes that line, in the format MIDNIGHT_FORMAT asks for, and ends
 *       as the runtime does. buffers_alive needs MN_DEBUG.
 */

#include "../../runtime/midnight_rt.c"

#include <pthread.h>
#include <unistd.h>

static void drop_nothing(void) {}

static const MnProgram program = {
    NULL, 0, NULL, 0, NULL, 0, drop_nothing, "test", NULL, 0,
};

static float float_of(uint32_t bits) {
    float value;
    memcpy(&value, &bits, sizeof value);
    return value;
}

/* Whether `text` reads back as `value` both ways. */
static bool both_ways(const char *text, float value) {
    float single = strtof(text, NULL);
    float rounded = (float)strtod(text, NULL);
    return single == value && rounded == value &&
           signbit(single) == signbit(value) && signbit(rounded) == signbit(value);
}

/* The number of significant digits of a text form. */
static int significant(const char *text) {
    char digits[40];
    int count = 0;
    for (const char *c = text; *c != '\0' && *c != 'e'; c++) {
        if (*c >= '0' && *c <= '9') digits[count++] = *c;
    }
    int first = 0;
    while (first < count && digits[first] == '0') first++;
    while (count > first && digits[count - 1] == '0') count--;
    return count > first ? count - first : 1;
}

/* Checks one pattern; on a failure prints it and returns false. */
static bool check(uint32_t bits) {
    float value = float_of(bits);
    if (!isfinite(value)) return true;
    char text[MN_FLOAT_TEXT + 1];
    int len = format_number(value, true, text);
    text[len] = '\0';
    if (!both_ways(text, value)) {
        printf("0x%08x: `%s` does not read back both ways\n", (unsigned)bits, text);
        return false;
    }
    int digits = significant(text);
    for (int p = 1; p < digits; p++) {
        char shorter[40];
        snprintf(shorter, sizeof shorter, "%.*e", p - 1, (double)value);
        if (both_ways(shorter, value)) {
            printf("0x%08x: `%s` is shorter than `%s`\n", (unsigned)bits, shorter, text);
            return false;
        }
    }
    return true;
}

typedef struct {
    const uint32_t *patterns; /* NULL for every pattern */
    uint64_t first;
    uint64_t end;
    uint64_t checked;
    bool failed;
} Range;

static void *check_range(void *argument) {
    Range *range = argument;
    for (uint64_t i = range->first; i < range->end && !range->failed; i++) {
        uint32_t bits = range->patterns ? range->patterns[i] : (uint32_t)i;
        if (!check(bits)) range->failed = true;
        range->checked++;
    }
    return NULL;
}

static int float32(bool all, unsigned threads) {
    uint32_t *patterns = NULL;
    uint64_t count = UINT64_C(1) << 32;
    if (!all) {
        count = 0;
        patterns = mn_malloc((1000000 + 600) * sizeof(uint32_t));
        for (uint32_t exponent = 0; exponent < 255; exponent++) {
            patterns[count++] = exponent << 23;
            patterns[count++] = (exponent << 23) | 0x80000000u;
        }
        for (uint32_t shift = 0; shift < 23; shift++) {
            patterns[count++] = 1u << shift;
            patterns[count++] = (1u << shift) | 0x80000000u;
        }
        float special = 7.0385307e-26f;
        uint32_t bits;
        memcpy(&bits, &special, sizeof bits);
        patterns[count++] = bits;
        patterns[count++] = bits | 0x80000000u;
        uint64_t state = 0x9e3779b97f4a7c15u;
        for (int i = 0; i < 1000000; i++) {
            state = state * 6364136223846793005u + 1442695040888963407u;
            patterns[count++] = (uint32_t)(state >> 32);
        }
    }
    if (threads < 1) threads = 1;
    Range *ranges = mn_calloc(threads, sizeof(Range));
    pthread_t *ids = mn_calloc(threads, sizeof(pthread_t));
    for (unsigned t = 0; t < threads; t++) {
        ranges[t].patterns = patterns;
        ranges[t].first = count * t / threads;
        ranges[t].end = count * (t + 1) / threads;
        pthread_create(&ids[t], NULL, check_range, &ranges[t]);
    }
    uint64_t checked = 0;
    bool failed = false;
    for (unsigned t = 0; t < threads; t++) {
        pthread_join(ids[t], NULL);
        checked += ranges[t].checked;
        failed = failed || ranges[t].failed;
    }
    printf("checked %llu patterns\n", (unsigned long long)checked);
    free(ranges);
    free(ids);
    free(patterns);
    return failed ? 1 : 0;
}

static _Noreturn void device_fail(bool window, const char *format, ...) {
    va_list args;
    va_start(args, format);
    mn_device_fail(window, format, args);
}

int main(int argc, char **argv) {
    const char *mode = argc > 1 ? argv[1] : "";
    if (strcmp(mode, "float32") == 0 && argc == 4) {
        return float32(strcmp(argv[2], "all") == 0, (unsigned)atoi(argv[3]));
    }
    mn_start(&program);
    if (strcmp(mode, "gpu_error") == 0) device_fail(false, "lost the %s", "device");
    if (strcmp(mode, "window_error") == 0) device_fail(true, "lost the \"window\"");
    if (strcmp(mode, "out_of_memory") == 0) mn_out_of_memory();
    if (strcmp(mode, "buffers_alive") == 0) {
        MnString kept = mn_str_new("kept", 4);
        (void)kept;
        mn_finish();
        return 0;
    }
    fprintf(stderr, "lines: unknown mode `%s`\n", mode);
    return 2;
}
