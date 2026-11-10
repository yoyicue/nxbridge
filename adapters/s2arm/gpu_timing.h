/* SPDX-License-Identifier: MIT */
#ifndef NXBRIDGE_S2ARM_GPU_TIMING_H
#define NXBRIDGE_S2ARM_GPU_TIMING_H
#include <math.h>
#include <stdint.h>

typedef struct { uint64_t safety_ns; int gpu_valid; } nxb_gpu_timing;

/* CPU bounds use the same host clock as Metal feedback. Actual execution must
 * fit inside them. An impossible GPU timestamp is not a performance sample;
 * use the whole host interval (including queue/callback waits) for safety.
 * Saturation preserves the caller's existing stop threshold without overflow.
 */
static inline nxb_gpu_timing nxb_gpu_timing_measure(double submitted, double callback,
                                                   double start, double end,
                                                   uint64_t limit_ns) {
    nxb_gpu_timing result = {limit_ns + 1, 0};
    if (!isfinite(submitted) || !isfinite(callback) || submitted <= 0 || callback < submitted)
        return result; /* No trustworthy bound: fail closed. */
    result.gpu_valid = isfinite(start) && isfinite(end) && start > 0 &&
        start >= submitted && end >= start && end <= callback && end < (double)UINT64_MAX / 1e9;
    double seconds = result.gpu_valid ? end - start : callback - submitted;
    if (seconds <= (double)limit_ns / 1e9)
        result.safety_ns = (uint64_t)ceil(seconds * 1e9);
    return result;
}
#endif
