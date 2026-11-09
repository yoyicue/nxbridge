#!/usr/bin/env python3
"""Single-peer UDP fault proxy for an explicitly isolated control-stream test.

Line commands on stdin: drop, resume, stop. JSON event timestamps use the
host's CLOCK_MONOTONIC. Packet contents and client addresses are never logged.
The upstream must be loopback; this is not a general LAN relay.
"""
import argparse
import ipaddress
import json
import selectors
import socket
import sys
import time


def endpoint(value):
    host, port = value.rsplit(':', 1)
    ip = ipaddress.ip_address(host)
    port = int(port)
    if ip.version != 4 or not 1024 <= port <= 65535:
        raise argparse.ArgumentTypeError('expected IPv4:port with port 1024–65535')
    return str(ip), port


def emit(event, **fields):
    print(json.dumps({'event': event, 'clock_ns': time.clock_gettime_ns(time.CLOCK_MONOTONIC), **fields}), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--listen', type=endpoint, required=True)
    parser.add_argument('--upstream', type=endpoint, required=True)
    parser.add_argument('--client', type=ipaddress.ip_address, required=True)
    args = parser.parse_args()
    if not ipaddress.ip_address(args.upstream[0]).is_loopback or args.client.version != 4:
        parser.error('upstream must be IPv4 loopback and client must be IPv4')
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as public, socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as backend, selectors.DefaultSelector() as poll:
        public.bind(args.listen); public.setblocking(False)
        backend.bind(('127.0.0.1', 0)); backend.connect(args.upstream); backend.setblocking(False)
        poll.register(public, selectors.EVENT_READ, 'public')
        poll.register(backend, selectors.EVENT_READ, 'backend')
        poll.register(sys.stdin, selectors.EVENT_READ, 'command')
        peer = None; dropping = False; last_forwarded = None; dropped = 0
        emit('ready')
        while True:
            for key, _ in poll.select():
                if key.data == 'command':
                    command = sys.stdin.readline().strip()
                    if command == 'drop':
                        dropping = True; dropped = 0; emit('drop', last_forwarded_ns=last_forwarded)
                    elif command == 'resume':
                        dropping = False; emit('resume', dropped=dropped)
                    elif command in ('stop', ''):
                        emit('stopped'); return
                    else:
                        emit('invalid_command')
                elif key.data == 'public':
                    payload, sender = public.recvfrom(65535)
                    if sender[0] != str(args.client) or (peer is not None and sender != peer):
                        continue
                    peer = sender
                    if dropping:
                        dropped += 1
                    else:
                        backend.send(payload); last_forwarded = time.clock_gettime_ns(time.CLOCK_MONOTONIC)
                else:
                    payload = backend.recv(65535)
                    if peer is not None:
                        public.sendto(payload, peer)


if __name__ == '__main__':
    main()
