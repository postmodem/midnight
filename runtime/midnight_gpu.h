/*
 * What midnight_rt.c and midnight_gpu.c need from each other.
 *
 * midnight_gpu.c is the part of the runtime that talks to wgpu-native. It
 * is compiled only into programs that declare a GPU system, and only when
 * the compiler was given the flags to build it (MIDNIGHT_GPU_FLAGS).
 * MN_GPU is then defined and midnight_rt.c includes this header. Generated
 * code never does.
 *
 * Every column a GPU block names has, per table, a GPU buffer holding the
 * column's bytes, and one of three states: only the CPU copy is current,
 * only the GPU copy is, or both are. A column starts with the CPU copy
 * current. While the GPU copy is current, the rows a table gains are in
 * CPU memory only until a block uses the column, and single rows can be
 * newer in CPU memory, or brought back to it, without the whole column
 * changing state.
 */

#ifndef MIDNIGHT_GPU_H
#define MIDNIGHT_GPU_H

#include "midnight_rt.h"

/* ---- In midnight_rt.c ---------------------------------------------------- */

_Noreturn void mn_leave(int status);
_Noreturn void mn_out_of_memory(void);
void *mn_malloc(size_t size);
void *mn_calloc(size_t count, size_t size);
void *mn_realloc(void *memory, size_t size);

/* Component columns of one table copied to GPU memory and back since the
 * last `stats` line. Entity handles, uniform words, the error word and
 * snapshots are not counted. */
extern uint64_t mn_gpu_uploads;
extern uint64_t mn_gpu_downloads;

/* Set by `rows` in MIDNIGHT_GPU: a row always travels alone, whatever it
 * costs. */
extern bool mn_gpu_rows;

/* Rows of component columns written to a GPU copy other than by whole
 * columns, and rows brought back alone; submissions to the queue, and
 * waits for the device, of any kind. Since the last `stats` line. */
extern uint64_t mn_gpu_rows_up;
extern uint64_t mn_gpu_rows_down;
extern uint64_t mn_gpu_submissions;
extern uint64_t mn_gpu_waits;

/* Why strict blocks run on the CPU, as the end of their `info` line, or
 * NULL when they may run on the device. */
extern const char *mn_gpu_strict_refused;

/* ---- In midnight_gpu.c --------------------------------------------------- */

/* Chooses the device: the fastest adapter of the primary backends, with
 * the limits it offers. An adapter that runs on the CPU is taken only with
 * `software`. Returns the device's name for the `info` line, or NULL when
 * there is none. */
const char *mn_gpu_device_start(bool software);

/* Records a block's shader over every table its outer query matches and
 * returns NULL. The work is submitted later, unless the block is
 * `fallible`: then it is submitted at once, the program waits for it, and
 * a failing operation is reported as mn_gpu_run reports it. Both queries
 * are up to date and `uniform` holds the block's words from 2 up; words 0
 * and 1 are overwritten.
 *
 * When the block cannot run on the device, makes the CPU copy of every
 * column it uses current, and the only current one for those it writes,
 * and returns the reason as text that is valid until the next call.
 * `*state` is what the device keeps for the block, NULL at first. */
const char *mn_gpu_device_run(MnGpuBlock *block, uint32_t *uniform,
                              void **state);

/* Runs the strict check's shader on the device and writes its results to
 * `out`, as check->cpu does. Returns NULL, or why the device cannot run it.
 * Counts no submission and no wait. */
const char *mn_gpu_device_check(const MnGpuCheck *check, uint32_t *out);

/* Releases what mn_gpu_device_run kept for a block. */
void mn_gpu_device_forget(void *state);

/* Call before a row leaves a table, when the table's last row is about to
 * take its place in CPU memory. Patches the GPU copy of each column: the
 * last row's value is moved in GPU memory when CPU memory lacks it, and is
 * otherwise sent with the next block that uses the column. */
void mn_gpu_leave(MnTable *table, uint32_t row);

/* Lists for the next round trip, in every column of the table but those
 * in `skip`, the row's value when only the GPU copy has it. Call before a
 * row moves to another table; mn_gpu_fetch brings it back. */
void mn_gpu_want_row(MnTable *table, uint32_t row, const uint32_t *skip,
                     uint32_t skip_count);

/* Brings back everything listed, with one wait for the device at most. */
void mn_gpu_fetch(void);

/* Submits the work recorded so far without waiting. */
void mn_gpu_device_submit(void);

/* Submits the work recorded so far, and returns when the device has
 * finished everything submitted to it. */
void mn_gpu_device_wait(void);

/* Releases every column copy and the device. Call after
 * mn_gpu_device_forget has been called for every block. */
void mn_gpu_device_finish(void);

#endif
