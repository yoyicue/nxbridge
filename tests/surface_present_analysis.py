#!/usr/bin/env python3
"""Summarize sampled SurfaceFlinger presentation timestamps, not game FPS."""
import argparse
import json
import math
import statistics
from pathlib import Path

PENDING = (1 << 63) - 1


def analyze(document):
    snapshots = document['snapshots']
    if len(snapshots) < 2:
        raise ValueError('need baseline and at least one later snapshot')
    def present(snapshot):
        return {row[1] for row in snapshot['rows'] if len(row) == 3 and 0 < row[1] < PENDING}
    previous = present(snapshots[0])
    if not previous:
        raise ValueError('baseline has no valid presentation timestamp')
    baseline = max(previous)
    frames = set()
    gaps = []
    for index, snapshot in enumerate(snapshots[1:], 1):
        current = present(snapshot)
        if not current or not current.intersection(previous):
            gaps.append(index)
        frames.update(x for x in current if x > baseline)
        previous = current
    ordered = sorted(frames)
    intervals = sorted((b-a)/1e6 for a, b in zip(ordered, ordered[1:]) if b > a)
    def quantile(q):
        return intervals[math.ceil(q * len(intervals)) - 1] if intervals else None
    fps = (len(ordered)-1)*1e9/(ordered[-1]-ordered[0]) if len(ordered) > 1 else None
    return {'schema': 1, 'mode': document['mode'], 'frame_count': len(ordered),
            'interval_count': len(intervals), 'snapshot_gaps': gaps,
            'present_fps': fps, 'interval_ms': {'p50': statistics.median(intervals) if intervals else None,
            'p95_nearest_rank': quantile(.95), 'p99_nearest_rank': quantile(.99),
            'maximum': intervals[-1] if intervals else None},
            'scope': 'SurfaceFlinger video-layer presentation; not game FPS'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    result = analyze(json.loads(args.input.read_text()))
    with args.output.open('x') as out:
        json.dump(result, out, indent=2); out.write('\n')
    print(result['mode'], 'frames', result['frame_count'], 'snapshot gaps', len(result['snapshot_gaps']))
    return 1 if result['snapshot_gaps'] else 0


if __name__ == '__main__':
    raise SystemExit(main())
