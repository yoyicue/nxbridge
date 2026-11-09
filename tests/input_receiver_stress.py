#!/usr/bin/env python3
"""Diagnostic: bounded loopback-fragment and heartbeat stress of the real C receiver.

This is not a game, network, device, or latency acceptance test. It starts two
owned CPU-busy subprocesses for a limited duration and records raw observations
in a new, explicitly named JSON file. It does not touch games or their data.
"""
import argparse
import datetime
import importlib.util
import json
import random
import socket
import subprocess
import sys
import time
from pathlib import Path

here = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('receiver_tests', here / 'input_receiver_test.py')
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True, help='new JSON results path')
    parser.add_argument('--cycles', type=int, default=100, choices=range(1, 151))
    args = parser.parse_args()
    if args.output.exists() or args.output.is_symlink():
        parser.error('output already exists')
    if not args.output.parent.is_dir():
        parser.error('output parent does not exist')
    rng = random.Random(120390)
    busy_seconds = args.cycles * .2 + 6
    code = ('import time\nstart=time.process_time();end=time.monotonic()+' + repr(busy_seconds) +
            '\nx=1\nwhile time.monotonic()<end:x=(x*1664525+1013904223)&0xffffffff\n'
            'print(time.process_time()-start,flush=True)\n')
    workers = []
    result = dict(schema=1, test='real-c-receiver-loopback', started_at=datetime.datetime.now().astimezone().isoformat(),
                  cycles_requested=args.cycles, heartbeat_period_ms=200, max_planned_fragment_delay_ms=140,
                  seed=120390, cpu_workers=2, cycles=[], tap_samples=[], partial_faults=[], cpu_seconds=[], failure=None)
    receiver = None
    try:
        for _ in range(2):
            workers.append(subprocess.Popen([sys.executable, '-c', code], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True))
        mod.InputReceiverTest.setUpClass()
        receiver = mod.InputReceiverTest(methodName='runTest'); receiver.setUp()
        held = (0x0101, 12000, -4000, 2000, -1000)
        receiver.s.sendall(receiver.frame(held[0], held[1:])); receiver.settle()
        if receiver.sample() != held: raise RuntimeError('initial held snapshot did not arrive')
        started = time.monotonic()
        for i in range(args.cycles):
            due = started + i * .2
            remaining = due - time.monotonic()
            if remaining > 0: time.sleep(remaining)
            target = (held[0], 8000 + (i * 137) % 10000, held[2], held[3], held[4])
            frame = receiver.frame(target[0], target[1:])
            split = rng.randint(1, len(frame) - 1)
            planned = rng.uniform(.005, .14)
            t0 = time.monotonic_ns(); receiver.s.sendall(frame[:split])
            before_tail = receiver.sample()
            remaining = planned - (time.monotonic_ns() - t0) / 1e9
            if remaining > 0: time.sleep(remaining)
            t1 = time.monotonic_ns(); receiver.s.sendall(frame[split:])
            deadline = time.monotonic() + .075
            sample = receiver.sample()
            while sample != target and time.monotonic() < deadline:
                time.sleep(.002); sample = receiver.sample()
            t2 = time.monotonic_ns()
            item = dict(cycle=i + 1, split_bytes=split, planned_delay_ms=planned * 1000,
                        observed_sender_delay_ms=(t1-t0)/1e6, receive_sample_delay_ms=(t2-t1)/1e6,
                        start_lateness_ms=max(0, (t0 - int(due * 1e9))/1e6),
                        before_tail=list(before_tail), target=list(target), sample=list(sample),
                        busy_workers_alive=sum(p.poll() is None for p in workers))
            result['cycles'].append(item)
            if before_tail != held: raise RuntimeError(f'partial frame changed prior state at cycle {i+1}')
            if sample != target: raise RuntimeError(f'complete frame not observed at cycle {i+1}')
            if item['busy_workers_alive'] != 2: raise RuntimeError('CPU workers not both active')
            held = target
        # Preserve button edges, with an intentional short gap between packets.
        for buttons in (0, 1, 0, 1, 0):
            receiver.s.sendall(receiver.frame(buttons)); time.sleep(.005)
        time.sleep(.08)
        result['tap_samples'] = [list(receiver.sample()) for _ in range(5)]
        if [x[0] for x in result['tap_samples']] != [0, 1, 0, 1, 0]:
            raise RuntimeError('short-tap edge sequence differs from expected')
        for i in range(5):
            receiver.s.sendall(receiver.frame(0x0100, (9000,0,0,0))); time.sleep(.06)
            if receiver.sample() != (0x0100,9000,0,0,0): raise RuntimeError('fault setup did not hold')
            frame = receiver.frame(2, (12000,0,0,0))
            receiver.s.sendall(frame[:12]); t0 = time.monotonic_ns(); deadline = time.monotonic() + .8
            value = receiver.sample()
            while value != (0,0,0,0,0) and time.monotonic() < deadline:
                time.sleep(.005); value = receiver.sample()
            t1 = time.monotonic_ns()
            item = dict(iteration=i+1, partial_to_neutral_sample_ms=(t1-t0)/1e6, sample=list(value),
                        busy_workers_alive=sum(p.poll() is None for p in workers))
            result['partial_faults'].append(item)
            if value != (0,0,0,0,0): raise RuntimeError('stalled partial failed to neutralize')
            if item['busy_workers_alive'] != 2: raise RuntimeError('CPU workers not active during fault')
            try: receiver.s.sendall(frame[12:])
            except OSError: pass
            time.sleep(.03)
            if receiver.sample() != (0,0,0,0,0): raise RuntimeError('late tail revived input')
            receiver.s.close(); time.sleep(.03)
            receiver.s = socket.create_connection(('127.0.0.1', receiver.port), timeout=1)
            receiver.s.sendall(mod.TOKEN); receiver.seq = 0
            receiver.s.sendall(receiver.frame(4, (500,0,0,0))); time.sleep(.06)
            if receiver.sample() != (4,500,0,0,0): raise RuntimeError('fresh connection input failed')
            receiver.s.sendall(receiver.frame(0)); time.sleep(.06)
            if receiver.sample() != (0,0,0,0,0): raise RuntimeError('fresh connection release failed')
    except Exception as e:
        result['failure'] = repr(e)
    finally:
        if receiver is not None:
            try: receiver.tearDown()
            except Exception as e: result['failure'] = result['failure'] or f'cleanup: {e!r}'
        if hasattr(mod.InputReceiverTest, 'tmp'):
            mod.InputReceiverTest.tearDownClass()
        for worker in workers:
            try: out, err = worker.communicate(timeout=8)
            except subprocess.TimeoutExpired:
                worker.terminate(); out, err = worker.communicate(timeout=3)
                result['failure'] = result['failure'] or 'CPU worker did not exit normally'
            if worker.returncode == 0:
                try: result['cpu_seconds'].append(float(out.strip()))
                except ValueError: result['failure'] = result['failure'] or 'invalid CPU worker sample'
            else: result['failure'] = result['failure'] or f'CPU worker exit {worker.returncode}'
        result['finished_at'] = datetime.datetime.now().astimezone().isoformat()
        with args.output.open('x') as out: json.dump(result, out, indent=2); out.write('\n')
    print(f"cycles {len(result['cycles'])}/{args.cycles}; workers {len(result['cpu_seconds'])}; failure {result['failure']}")
    return 0 if result['failure'] is None else 1

if __name__ == '__main__': sys.exit(main())
