#!/usr/bin/env python3
"""Behavioral checks of the real receiver over its loopback protocol."""
import os
import pathlib
import socket
import struct
import subprocess
import tempfile
import time
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
TOKEN = bytes(range(32))

class InputReceiverTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        harness = pathlib.Path(cls.tmp.name) / 'harness.c'
        harness.write_text('''#include <stdint.h>\n#include <stdio.h>\n#include "nxbridge_input.h"\nint main(void) { char s[16]; while (fgets(s,sizeof s,stdin)) { uint64_t b=0; int32_t a[4]={0}; nxbridge_input_sample(&b,a); printf("%llu %d %d %d %d\\n",(unsigned long long)b,a[0],a[1],a[2],a[3]); fflush(stdout); } }\n''')
        cls.binary = pathlib.Path(cls.tmp.name) / 'receiver'
        subprocess.run(['clang', '-std=c11', '-D_POSIX_C_SOURCE=200809L', '-pthread', '-I', str(ROOT / 'adapters/input'), str(ROOT / 'adapters/input/nxbridge_input.c'), str(harness), '-o', str(cls.binary)], check=True)

    @classmethod
    def tearDownClass(cls): cls.tmp.cleanup()

    def setUp(self):
        with socket.socket() as s:
            s.bind(('127.0.0.1', 0)); port = s.getsockname()[1]
        self.port = port
        env = dict(os.environ, NXBRIDGE_INPUT_PORT=str(port), NXBRIDGE_INPUT_TOKEN=TOKEN.hex())
        self.proc = subprocess.Popen([str(self.binary)], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        for _ in range(100):
            try:
                self.s = socket.create_connection(('127.0.0.1', port), timeout=1); break
            except ConnectionRefusedError: time.sleep(.01)
        else: self.fail('listener did not start')
        self.s.sendall(TOKEN)
        self.seq = 0

    def tearDown(self):
        self.s.close(); self.proc.terminate(); self.proc.wait(timeout=3); self.proc.stdin.close(); self.proc.stdout.close()

    def frame(self, buttons=0, axes=(0,0,0,0), seq=None):
        self.seq += 1
        return struct.pack('!4sQHhhhh', b'NXI1', self.seq if seq is None else seq, buttons, *axes)

    def sample(self):
        self.proc.stdin.write('s\n'); self.proc.stdin.flush()
        return tuple(map(int, self.proc.stdout.readline().split()))

    def settle(self): time.sleep(.06)

    def test_fragmented_short_tap_and_axes(self):
        frame = self.frame(0x1041, (12345,-22000,7,32767))
        self.s.sendall(frame[:3]); self.s.sendall(frame[3:]); self.s.sendall(self.frame(0, (0,0,0,0)))
        self.settle()
        self.assertEqual(self.sample(), (0x1041,0,0,0,0))
        self.assertEqual(self.sample(), (0,0,0,0,0))
        self.s.sendall(self.frame(0x0802, (12345,-22000,7,32767))); self.settle()
        self.assertEqual(self.sample(), (0x0802,12345,-22000,7,32767))
        self.assertEqual(self.sample(), (0x0802,12345,-22000,7,32767))

    def test_disconnect_clears_held_and_unconsumed_edges(self):
        self.s.sendall(self.frame(1, (100,0,0,0))); self.settle(); self.s.close(); self.settle()
        self.assertEqual(self.sample(), (0,0,0,0,0))

    def test_timeout_clears(self):
        self.s.sendall(self.frame(1, (100,0,0,0))); self.settle()
        self.assertEqual(self.sample()[0], 1)
        time.sleep(1.1)
        self.assertEqual(self.sample(), (0,0,0,0,0))

    def test_reordered_frame_clears(self):
        self.s.sendall(self.frame(1)); self.settle()
        self.s.sendall(self.frame(2, seq=1)); self.settle()
        self.assertEqual(self.sample(), (0,0,0,0,0))

    def test_invalid_auth_cannot_inject(self):
        self.s.close(); self.settle()
        with socket.create_connection(self.proc_address()) as attacker:
            attacker.sendall(bytes(32) + self.frame(0xffff)); self.settle()
        self.assertEqual(self.sample(), (0,0,0,0,0))

    def proc_address(self):
        # The current connection still records the listener's address after close
        return ('127.0.0.1', self.port)

if __name__ == "__main__": unittest.main()
