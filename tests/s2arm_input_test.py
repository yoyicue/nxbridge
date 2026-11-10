#!/usr/bin/env python3
"""Exercise S2Arm's exported input hook through the actual session receiver."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import input_receiver_test as receiver_tests

ROOT = Path(__file__).resolve().parents[1]


class S2ArmInputTest(receiver_tests.InputReceiverTest):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        harness = Path(cls.tmp.name) / 'harness.c'
        harness.write_text('''
#include <stdint.h>
#include <stdio.h>
uint32_t s2a_vpad_read(int32_t *);
int main(void) {
    char line[16];
    while (fgets(line, sizeof line, stdin)) {
        int32_t axes[4] = {30000, -30000, 1234, -1234};
        uint32_t buttons = s2a_vpad_read(axes);
        printf("%u %d %d %d %d\\n", buttons, axes[0], axes[1], axes[2], axes[3]);
        fflush(stdout);
    }
}
''')
        cls.binary = Path(cls.tmp.name) / 's2arm-input'
        subprocess.run([
            'clang', '-std=c11', '-D_POSIX_C_SOURCE=200809L', '-pthread',
            '-Wall', '-Wextra', '-Werror', '-I', str(ROOT / 'adapters/input'),
            str(ROOT / 'adapters/input/nxbridge_input.c'),
            str(ROOT / 'adapters/s2arm/input.c'), str(harness), '-o', str(cls.binary),
        ], check=True)

    def test_bad_config_fails_closed_but_unconfigured_host_keeps_local_input(self):
        # This headless hook owns no keyboard or physical controller. Even an
        # unconfigured process must replace the caller's stale axes with neutral.
        base = {k: v for k, v in os.environ.items() if not k.startswith('NXBRIDGE_INPUT_')}
        for extra in ({}, {'NXBRIDGE_INPUT_PORT': 'bad'}, {'NXBRIDGE_INPUT_TOKEN': 'bad'}):
            result = subprocess.run([str(self.binary)], env={**base, **extra},
                                    input='p\n', capture_output=True, text=True, timeout=3)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), '0 0 0 0 0')


if __name__ == '__main__':
    unittest.main()
