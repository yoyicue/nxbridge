#!/usr/bin/env python3
"""Analyze Android gfxinfo framestats for one named window, not video/game FPS."""
import argparse
import csv
import json
import math
import statistics
from pathlib import Path


def stats(values):
    values = sorted(values)
    if not values:
        return None
    return {'count': len(values), 'min': values[0], 'p50': statistics.median(values),
            'p95_nearest_rank': values[math.ceil(.95*len(values))-1],
            'p99_nearest_rank': values[math.ceil(.99*len(values))-1], 'max': values[-1]}


def analyze(text, window):
    in_window = False
    in_profile = False
    header = None
    durations = []
    skipped = 0
    for line in text.splitlines():
        if line.startswith('Window: '):
            in_window = line[len('Window: '):].strip() == window
            in_profile = False
            header = None
        elif in_window and line.strip() == '---PROFILEDATA---':
            in_profile = not in_profile
            header = None
        elif in_window and in_profile:
            fields = next(csv.reader([line]))
            if not fields or fields == ['']:
                continue
            if header is None:
                header = fields
                if not {'Flags', 'IntendedVsync', 'FrameCompleted'}.issubset(header):
                    raise ValueError('unexpected framestats header')
                continue
            try:
                row = dict(zip(header, fields))
                flags = int(row['Flags']); start = int(row['IntendedVsync']); end = int(row['FrameCompleted'])
                if flags != 0 or start <= 0 or end < start or end >= (1 << 63)-1:
                    skipped += 1
                else:
                    durations.append((end-start)/1e6)
            except (ValueError, KeyError):
                skipped += 1
    return {'schema': 1, 'window': window, 'frame_duration_ms': stats(durations),
            'valid_frames': len(durations), 'skipped_rows': skipped,
            'scope': 'UI frame IntendedVsync to FrameCompleted; not video or game FPS'}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input', required=True, type=Path);p.add_argument('--window', required=True);p.add_argument('--output', required=True, type=Path)
    a=p.parse_args();r=analyze(a.input.read_text(errors='replace'),a.window)
    with a.output.open('x') as out:json.dump(r,out,indent=2);out.write('\n')
    print('UI frames',r['valid_frames'],'skipped',r['skipped_rows'])


if __name__ == '__main__':main()
