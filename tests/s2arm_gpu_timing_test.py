#!/usr/bin/env python3
"""Run the actual S2Arm timing/stop functions with bounded synthetic feedback.

No GPU work or deliberate GPU hang. --engine names a prepared checkout.
The old upstream functions compile too, providing a meaningful negative control.
"""
import argparse
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def function(source, name):
    match = re.search(r'^.*\b' + name + r'\([^\n]*\) \{', source, re.M)
    if not match:
        raise ValueError(f'missing function: {name}')
    end = source.index('\n}', match.end()) + 2
    return source[match.start():end]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--engine', type=Path, required=True)
    args = parser.parse_args()
    source = (args.engine / 'graphics/nvn/gfx/nvn/nvn_mtl.m').read_text()
    done = function(source, 'gpu_tag_done')
    call = 'gpu_tag_done(frame, start, end' + (', submitted' if 'double submitted' in done.split('{')[0] else '') + ');'
    declarations = re.search(r'^static struct \{.*\} g_gpu\[GRING\];.*$', source, re.M).group(0)
    body = r'''
#include <assert.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdatomic.h>
#include <string.h>
#if __has_include("gpu_timing.h")
#include "gpu_timing.h"
#endif
#define GRING 1024
#define S2A_GPU_FRAME_MAX_MS 1000
#define MLOG(...) ((void)0)
static int halted, checks;
static double now;
static double CACurrentMediaTime(void) { return now; }
static void mtl_gpu_halt(const char *why) { (void)why; halted = 1; }
#define CHECK(expr) do { checks++; if (!(expr)) { fprintf(stderr, "FAIL line %d: %s\n", __LINE__, #expr); return 1; } } while(0)
'''
    body += declarations + '\n'
    for name in ['gpu_tag_begin', 'gpu_tag_done', 'nvn_be_gpu_ms', 'nvn_be_gpu_span']:
        body += function(source, name) + '\n'
    body += '''static void feedback(uint64_t frame, double submitted, double start, double end, double callback) {
        now = callback; gpu_tag_begin(frame); ''' + call + '''
    }
int main(void) {
    uint64_t t0, t1;
    // Observed OoT feedback: 2.461 s reported within a 13.827 ms host interval.
    feedback(1, 781673.298016000, 781670.850820500, 781673.311693000, 781673.311843083);
    CHECK(!halted);
    CHECK(nvn_be_gpu_ms(1) == -1);
    CHECK(!nvn_be_gpu_span(1, &t0, &t1));
    // Genuine GPU time stays unchanged and remains usable by the profiler.
    feedback(2, 10, 10.01, 10.03, 10.04);
    CHECK(!halted);
    CHECK(fabs(nvn_be_gpu_ms(2) - 20) < 0.001);
    CHECK(nvn_be_gpu_span(2, &t0, &t1) && t1 - t0 > 19999000 && t1 - t0 < 20001000);
    // A real long GPU submission must still stop at the existing threshold.
    feedback(3, 20, 20.01, 21.11, 21.12); CHECK(halted); halted = 0;
    // So must the sum of separate valid submissions from one frame.
    feedback(4, 30, 30.01, 30.61, 30.62); CHECK(!halted);
    feedback(4, 31, 31.01, 31.61, 31.62); CHECK(halted); halted = 0;
    // Invalid metadata is not permission to skip the safety guard.
    feedback(5, 40, 0, 42, 42); CHECK(halted); halted = 0;
    feedback(6, 50, 49, 51.1, 51.2); CHECK(halted); halted = 0;
    // Two invalid but short intervals still add toward the frame guard.
    feedback(7, 60, 59, 60.59, 60.6); CHECK(!halted);
    feedback(7, 61, 59, 61.59, 61.6); CHECK(halted); halted = 0;
    // Other impossible metadata makes metrics unknown; never cast NaN to uint64.
    feedback(8, 70, 70.01, 72, 70.2); CHECK(!halted && nvn_be_gpu_ms(8) == -1);
    feedback(9, 80, NAN, 80.1, 80.2); CHECK(!halted && nvn_be_gpu_ms(9) == -1);
    feedback(10, 90, 90.1, INFINITY, 90.2); CHECK(!halted && nvn_be_gpu_ms(10) == -1);
    feedback(11, 100, 100.2, 100.1, 100.3); CHECK(!halted && nvn_be_gpu_ms(11) == -1);
    // If even the host bounds are unusable, fail closed.
    feedback(12, NAN, 110.1, 110.2, 110.3); CHECK(halted); halted = 0;
    feedback(13, 121, 120.1, 120.2, 120.3); CHECK(halted); halted = 0;
    // Ring reuse clears the previous frame's unknown flag and safety total.
    feedback(1025, 130, 130.01, 130.03, 130.04);
    CHECK(!halted && fabs(nvn_be_gpu_ms(1025) - 20) < 0.001);
    CHECK(nvn_be_gpu_span(1025, &t0, &t1));
    CHECK(nvn_be_gpu_ms(1) == -1);
    printf("S2Arm GPU feedback: %d checks passed; no GPU workload submitted\\n", checks);
    return 0;
}
'''
    with tempfile.TemporaryDirectory(prefix='nxbridge-gpu-time-') as tmp:
        path = Path(tmp)
        (path / 'test.c').write_text(body)
        subprocess.run(['cc', '-std=c11', '-Wall', '-Wextra', '-Wno-unused-function',
                        '-Wno-unused-parameter', '-fsanitize=undefined',
                        '-I', str(ROOT / 'adapters/s2arm'), str(path / 'test.c'),
                        '-lm', '-o', str(path / 'test')], check=True)
        return subprocess.run([str(path / 'test')], timeout=5).returncode


if __name__ == '__main__':
    raise SystemExit(main())
