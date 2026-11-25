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
 * current.
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

/* Rows of component columns written to a GPU copy other than by whole
 * columns, and rows brought back alone; submissions to the queue, and
 * waits for the device, of any kind. Since the last `stats` line. */
extern uint64_t mn_gpu_rows_up;
extern uint64_t mn_gpu_rows_down;
extern uint64_t mn_gpu_submissions;
extern uint64_t mn_gpu_waits;

/* ---- In midnight_gpu.c --------------------------------------------------- */

/* Chooses the device: the fastest adapter of the primary backends, with
 * the limits it offers. An adapter that runs on the CPU is taken only with
 * `software`. Returns the device's name for the `info` line, or NULL when
 * there is none. */
const char *mn_gpu_device_start(bool software);

/* Runs a block's shader over every table its outer query matches and
 * returns NULL; a failing operation is reported as mn_gpu_run reports it.
 * Both queries are up to date and `uniform` holds the block's words from 2
 * up; words 0 and 1 are overwritten.
 *
 * When the block cannot run on the device, makes the CPU copy of every
 * column it uses current, and the only current one for those it writes,
 * and returns the reason as text that is valid until the next call.
 * `*state` is what the device keeps for the block, NULL at first. */
const char *mn_gpu_device_run(MnGpuBlock *block, uint32_t *uniform,
                              void **state);

/* Releases what mn_gpu_device_run kept for a block. */
void mn_gpu_device_forget(void *state);

/* Makes the CPU copy of every column of a table the only current one.
 * Call before the table's rows change. */
void mn_gpu_reclaim(MnTable *table);

/* Returns when the device has finished everything submitted to it. */
void mn_gpu_device_wait(void);

/* Releases every column copy and the device. Call after
 * mn_gpu_device_forget has been called for every block. */
void mn_gpu_device_finish(void);

#endif
