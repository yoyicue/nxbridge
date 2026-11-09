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
        harness.write_text('''#include <stdint.h>\n#include <stdio.h>\n#include "nxbridge_input.h"\nint main(void) { char s[16]; while (fgets(s,sizeof s,stdin)) { uint64_t b=s[0]=='p' ? 0x8000 : 0; int32_t a[4]={s[0]=='p' ? 30000 : 0,0,0,0}; nxbridge_input_sample(&b,a); nxbridge_input_trace(b,a); printf("%llu %d %d %d %d\\n",(unsigned long long)b,a[0],a[1],a[2],a[3]); fflush(stdout); } }\n''')
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

    def reset_frame(self, buttons=0, axes=(0,0,0,0)):
        self.seq += 1
        return struct.pack('!4sQHhhhh', b'NXIR', self.seq, buttons, *axes)

    def sample(self, local_physical=False):
        self.proc.stdin.write('p\n' if local_physical else 's\n'); self.proc.stdin.flush()
        return tuple(map(int, self.proc.stdout.readline().split()))

    def settle(self): time.sleep(.06)

    def test_fragmented_short_tap_and_axes(self):
        frame = self.frame(0x1041, (12345,-22000,7,32767))
        self.s.sendall(frame[:3]); self.s.sendall(frame[3:]); self.s.sendall(self.frame(0, (0,0,0,0)))
        self.settle()
        self.assertEqual(self.sample(), (0x1041,12345,-22000,7,32767))
        self.assertEqual(self.sample(), (0,0,0,0,0))
        self.s.sendall(self.frame(0x0802, (12345,-22000,7,32767))); self.settle()
        self.assertEqual(self.sample(), (0x0802,12345,-22000,7,32767))
        self.assertEqual(self.sample(), (0x0802,12345,-22000,7,32767))

    def test_incomplete_frame_does_not_publish_partial_state(self):
        self.s.sendall(self.frame(1, (12000, 0, 0, 0)))
        self.settle()
        self.assertEqual(self.sample(), (1, 12000, 0, 0, 0))
        next_frame = self.frame(2, (-14000, 3000, 0, 0))
        self.s.sendall(next_frame[:12])
        time.sleep(.14)  # A legitimately delayed fragment remains within the new deadline.
        # Bytes from a partial frame must not alter the last complete snapshot.
        self.assertEqual(self.sample(), (1, 12000, 0, 0, 0))
        self.s.sendall(next_frame[12:])
        self.settle()
        self.assertEqual(self.sample(), (2, -14000, 3000, 0, 0))

    def test_truncated_frame_timeout_clears_and_new_connection_starts_clean(self):
        self.s.sendall(self.frame(1, (12000, 0, 0, 0)))
        self.settle()
        self.assertEqual(self.sample(), (1, 12000, 0, 0, 0))
        partial = self.frame(2, (-14000, 3000, 0, 0))
        self.s.sendall(partial[:12])
        time.sleep(.55)  # Allow scheduling margin around the 200 ms fragment deadline.
        self.assertEqual(self.sample(), (0, 0, 0, 0, 0))
        # A continuation of the rejected frame must not revive the old state.
        try:
            self.s.sendall(partial[12:])
        except OSError:
            pass
        self.settle()
        self.assertEqual(self.sample(), (0, 0, 0, 0, 0))
        self.s.close()
        self.s = socket.create_connection(('127.0.0.1', self.port), timeout=1)
        self.s.sendall(TOKEN)
        self.seq = 0
        self.s.sendall(self.frame(4, (500, 0, 0, 0)))
        self.settle()
        self.assertEqual(self.sample(), (4, 500, 0, 0, 0))

    def test_fragment_deadline_is_not_extended_by_more_partial_bytes(self):
        self.s.sendall(self.frame(1, (12000, 0, 0, 0)))
        self.settle()
        self.assertEqual(self.sample(), (1, 12000, 0, 0, 0))
        partial = self.frame(2)
        self.s.sendall(partial[:3])
        time.sleep(.1)
        self.s.sendall(partial[3:6])
        time.sleep(.45)
        self.assertEqual(self.sample(), (0, 0, 0, 0, 0))

    def test_normal_idle_heartbeat_does_not_trip_fragment_deadline(self):
        for _ in range(5):
            self.s.sendall(self.frame(1, (8000, 0, 0, 0)))
            time.sleep(.18)
            self.assertEqual(self.sample(), (1, 8000, 0, 0, 0))
        self.s.sendall(self.frame(0))
        self.settle()
        self.assertEqual(self.sample(), (0, 0, 0, 0, 0))

    def test_bad_config_fails_closed_but_unconfigured_host_keeps_local_input(self):
        env = dict(os.environ)
        env.pop('NXBRIDGE_INPUT_PORT', None); env.pop('NXBRIDGE_INPUT_TOKEN', None)
        result = subprocess.run([str(self.binary)], env=env, input='p\n', capture_output=True, text=True, check=True)
        self.assertEqual(tuple(map(int, result.stdout.split())), (0x8000,30000,0,0,0))
        env.update(NXBRIDGE_INPUT_PORT=str(self.port), NXBRIDGE_INPUT_TOKEN='bad')
        result = subprocess.run([str(self.binary)], env=env, input='p\n', capture_output=True, text=True, check=True)
        self.assertEqual(tuple(map(int, result.stdout.split())), (0,0,0,0,0))

    def test_unassigned_local_controller_is_not_merged_into_session(self):
        self.s.sendall(self.frame(1, (1000,0,0,0))); self.settle()
        self.assertEqual(self.sample(local_physical=True), (1,1000,0,0,0))
        self.s.sendall(self.frame(0)); self.settle()
        self.assertEqual(self.sample(local_physical=True), (0,0,0,0,0))

    def test_explicit_reset_flushes_unconsumed_edges_and_accepts_fresh_input(self):
        for b in (1,0,1): self.s.sendall(self.frame(b, (12000,0,0,0)))
        self.s.sendall(self.reset_frame()); self.settle()
        self.assertEqual(self.sample(), (0,0,0,0,0))
        self.s.sendall(self.frame(2, (10,0,0,0))); self.settle()
        self.assertEqual(self.sample(), (2,10,0,0,0))

    def test_non_neutral_reset_is_rejected_and_clears(self):
        self.s.sendall(self.frame(1)); self.settle()
        self.s.sendall(self.reset_frame(buttons=2)); self.settle()
        self.assertEqual(self.sample(), (0,0,0,0,0))

    def test_two_short_taps_keep_release_between_presses(self):
        for buttons in (1, 0, 1, 0): self.s.sendall(self.frame(buttons))
        self.settle()
        self.assertEqual([self.sample()[0] for _ in range(5)], [1, 0, 1, 0, 0])

    def test_short_combination_is_sampled_with_its_axes(self):
        self.s.sendall(self.frame(0x0101, (16000,-4000,0,0)))
        self.s.sendall(self.frame(0, (0,0,0,0)))
        self.settle()
        self.assertEqual(self.sample(), (0x0101,16000,-4000,0,0))
        self.assertEqual(self.sample(), (0,0,0,0,0))

    def test_queued_combinations_keep_their_own_four_axis_snapshot(self):
        # A quick sequence may arrive before the game asks for any state.
        # Preserve the axes paired with each button edge, then use the most
        # recent axis-only update after the edge backlog has drained.
        states = [
            (0x0100, (12000, -9000, 3000, -2000)),
            (0x0100, (16000, -5000, 5000, -4000)),
            (0x1101, (17000, -4000, 6000, -3000)),
            (0x1101, (21000, -1000, 7000, -2000)),
            (0x1001, (9000, 1000, 4000, 2000)),
            (0, (0, 0, 0, 0)),
            (0, (4000, 5000, -6000, -7000)),
        ]
        for buttons, axes in states:
            self.s.sendall(self.frame(buttons, axes))
        self.settle()
        self.assertEqual([self.sample() for _ in range(5)], [
            (0x0100, 12000, -9000, 3000, -2000),
            (0x1101, 17000, -4000, 6000, -3000),
            (0x1001, 9000, 1000, 4000, 2000),
            (0, 0, 0, 0, 0),
            (0, 4000, 5000, -6000, -7000),
        ])

    def test_overflow_requires_all_buttons_and_all_axes_neutral(self):
        for i in range(9):
            self.s.sendall(self.frame(1 if i % 2 == 0 else 0, (10, 0, 0, 0)))
        self.settle()
        self.assertEqual(self.sample(), (0, 0, 0, 0, 0))
        # Releasing buttons while an axis is still displaced is not neutral.
        self.s.sendall(self.frame(0, (0, 0, 0, 1)))
        self.s.sendall(self.frame(2, (20, 0, 0, 0)))
        self.settle()
        self.assertEqual(self.sample(), (0, 0, 0, 0, 0))
        self.s.sendall(self.frame(0))
        self.s.sendall(self.frame(2, (20, 0, 0, 0)))
        self.settle()
        self.assertEqual(self.sample(), (2, 20, 0, 0, 0))

    def test_axis_only_uses_latest_state(self):
        for v in (1000, 2000, 3000): self.s.sendall(self.frame(0, (v,0,0,0)))
        self.settle()
        self.assertEqual(self.sample(), (0,3000,0,0,0))

    def test_overflow_neutralizes_until_fresh_neutral(self):
        for i in range(9): self.s.sendall(self.frame(1 if i % 2 == 0 else 0, (10,0,0,0)))
        self.settle()
        self.assertEqual(self.sample(), (0,0,0,0,0))
        self.s.sendall(self.frame(2, (20,0,0,0))); self.settle()
        self.assertEqual(self.sample(), (0,0,0,0,0))
        self.s.sendall(self.frame(0, (0,0,0,0))); self.s.sendall(self.frame(2, (20,0,0,0))); self.settle()
        self.assertEqual(self.sample(), (2,20,0,0,0))

    def test_disconnect_clears_held_and_unconsumed_edges(self):
        self.s.sendall(self.frame(1, (100,0,0,0))); self.settle(); self.s.close(); self.settle()
        self.assertEqual(self.sample(), (0,0,0,0,0))

    def test_timeout_clears(self):
        self.s.sendall(self.frame(1, (100,0,0,0))); self.settle()
        self.assertEqual(self.sample()[0], 1)
        time.sleep(1.1)
        self.assertEqual(self.sample(), (0,0,0,0,0))

    def test_optional_trace_uses_host_monotonic_clock(self):
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0)); port = sock.getsockname()[1]
        trace = pathlib.Path(self.tmp.name) / 'monotonic-trace.txt'
        env = dict(os.environ, NXBRIDGE_INPUT_PORT=str(port), NXBRIDGE_INPUT_TOKEN=TOKEN.hex(),
                   NXBRIDGE_INPUT_TRACE=str(trace), NXBRIDGE_INPUT_TRACE_MONOTONIC='1')
        proc = subprocess.Popen([str(self.binary)], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        try:
            for _ in range(100):
                try:
                    conn = socket.create_connection(('127.0.0.1', port), timeout=1); break
                except ConnectionRefusedError: time.sleep(.01)
            else: self.fail('timed receiver did not start')
            with conn:
                conn.sendall(TOKEN + self.frame(1)); self.settle()
                before = time.clock_gettime_ns(time.CLOCK_MONOTONIC)
                proc.stdin.write('s\n'); proc.stdin.flush()
                self.assertEqual(tuple(map(int, proc.stdout.readline().split())), (1,0,0,0,0))
                after = time.clock_gettime_ns(time.CLOCK_MONOTONIC)
                fields = trace.read_text().split()
                self.assertEqual(fields[0], 'M')
                self.assertLessEqual(before, int(fields[1])); self.assertLessEqual(int(fields[1]), after)
                self.assertEqual(tuple(map(int, fields[2:])), (1,0,0,0,0))
        finally:
            proc.terminate(); proc.wait(timeout=3); proc.stdin.close(); proc.stdout.close()

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
