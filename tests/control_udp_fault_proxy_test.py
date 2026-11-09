#!/usr/bin/env python3
"""Exercise forwarding, one-way fault injection and resumption on loopback."""
import json
from pathlib import Path
import socket
import subprocess
import sys

with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as backend, socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as client, socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as reserved:
    backend.bind(('127.0.0.1', 0)); backend.settimeout(.25)
    reserved.bind(('127.0.0.1', 0)); listen = reserved.getsockname()[1]; reserved.close()
    client.bind(('127.0.0.1', 0)); client.settimeout(.5)
    script = Path(__file__).with_name('control_udp_fault_proxy.py')
    proc = subprocess.Popen([sys.executable, str(script), '--listen', f'127.0.0.1:{listen}', '--upstream', f'127.0.0.1:{backend.getsockname()[1]}', '--client', '127.0.0.1'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    try:
        assert json.loads(proc.stdout.readline())['event'] == 'ready'
        client.sendto(b'first', ('127.0.0.1', listen))
        data, proxy = backend.recvfrom(1024); assert data == b'first'
        backend.sendto(b'reply', proxy); assert client.recv(1024) == b'reply'
        proc.stdin.write('drop\n'); proc.stdin.flush(); drop = json.loads(proc.stdout.readline())
        assert drop['event'] == 'drop' and drop['last_forwarded_ns'] <= drop['clock_ns']
        client.sendto(b'blocked', ('127.0.0.1', listen))
        try: backend.recvfrom(1024); raise AssertionError('packet crossed fault')
        except socket.timeout: pass
        proc.stdin.write('resume\n'); proc.stdin.flush(); assert json.loads(proc.stdout.readline())['dropped'] == 1
        client.sendto(b'after', ('127.0.0.1', listen)); assert backend.recvfrom(1024)[0] == b'after'
        proc.stdin.write('stop\n'); proc.stdin.flush(); assert json.loads(proc.stdout.readline())['event'] == 'stopped'
        assert proc.wait(timeout=3) == 0
        print('PASS: isolated control proxy forwards, drops and resumes')
    finally:
        if proc.poll() is None: proc.terminate(); proc.wait(timeout=3)
        proc.stdin.close(); proc.stdout.close(); proc.stderr.close()
