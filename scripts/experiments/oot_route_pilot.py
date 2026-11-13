#!/usr/bin/env python3
"""Bounded, screenshot-gated OoT room-to-Saria experiment; not a general player.

Requires a separately isolated OoT session, a connected Moonlight Pad, Pillow,
and private reference screenshots from the same game build and controller layout.
It never creates or resets saves, starts or stops services, or changes settings.
"""
import argparse
import io
import json
import subprocess
import time
from pathlib import Path

from PIL import Image, ImageChops, ImageStat

STAGES = [
    ('01-title', ('hold', 2395, 807, 250)),
    ('select-save', ('hold', 247, 1434, 250)),
    ('confirm', ('hold', 2395, 807, 250)),
    ('loading', ('hold', 2395, 807, 250)),
    ('room1', ('swipe', 600, 1200, 600, 1450, 3000)),
    ('room2', ('hold', 2395, 807, 250)),
    ('outside1', ('swipe', 600, 1200, 600, 900, 3000)),
    ('ladder1', ('swipe', 600, 1200, 600, 1450, 4000)),
    ('ladder2', ('swipe', 600, 1200, 600, 1450, 2500)),
    ('ground', ('swipe', 600, 1200, 600, 1450, 1800)),
    ('forest1', ('hold', 2395, 807, 250)),
    ('saria', None),
]
# The thresholds are conservative, empirical stop conditions for this one
# camera / language / layout, not proof of general image recognition accuracy.
THRESHOLDS = {'01-title': 27, 'select-save': 13, 'confirm': 13,
              'loading': 13, 'room1': 18, 'room2': 16, 'outside1': 23,
              'ladder1': 16, 'ladder2': 16, 'ground': 18,
              'forest1': 22, 'saria': 22}

def normalized(image):
    if image.size != (2560, 1600):
        raise ValueError('expected 2560x1600 landscape screenshot; layout must match references')
    return image.convert('RGB').crop((512, 240, 1920, 1200)).resize((72, 48))

def distance(a, b):
    return sum(ImageStat.Stat(ImageChops.difference(a, b)).mean) / 3

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--references', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--stage-timeout', type=int, default=90)
    args = parser.parse_args()
    if not 5 <= args.stage_timeout <= 180:
        parser.error('stage timeout must be between 5 and 180 seconds')
    refs = {name: normalized(Image.open(args.references / (name + '.png'))) for name, _ in STAGES}
    args.output.mkdir(mode=0o700, parents=False, exist_ok=False)
    adb = ['adb', '-s', args.serial]
    deadline_all = time.monotonic() + 20 * 60
    with (args.output / 'events.jsonl').open('x') as events:
        def emit(kind, **fields):
            record = {'kind': kind, 'wall_time': time.time(),
                      'monotonic_ns': time.clock_gettime_ns(time.CLOCK_MONOTONIC), **fields}
            events.write(json.dumps(record, ensure_ascii=False) + '\n')
            events.flush()
            print(json.dumps(record, ensure_ascii=False), flush=True)
        def game_foreground():
            state = subprocess.check_output(adb + ['shell', 'dumpsys', 'activity', 'activities'], text=True, timeout=8)
            return any('topResumedActivity=' in line and 'com.limelight.debug/com.limelight.Game}' in line for line in state.splitlines())
        try:
            for index, (name, action) in enumerate(STAGES):
                deadline = min(time.monotonic() + args.stage_timeout, deadline_all)
                best = None
                while time.monotonic() < deadline:
                    if not game_foreground():
                        raise RuntimeError('Moonlight game activity is no longer foreground')
                    raw = subprocess.check_output(adb + ['exec-out', 'screencap', '-p'], timeout=12)
                    with Image.open(io.BytesIO(raw)) as img:
                        score = distance(normalized(img), refs[name])
                    best = score if best is None else min(best, score)
                    if score <= THRESHOLDS[name]:
                        (args.output / f'{index:02d}-{name}.png').write_bytes(raw)
                        emit('checkpoint', name=name, difference=round(score, 3), threshold=THRESHOLDS[name])
                        break
                    time.sleep(1)
                else:
                    if 'raw' in locals():
                        (args.output / 'timeout-last.png').write_bytes(raw)
                    raise TimeoutError(f'checkpoint {name} not recognized; best difference={best}')
                if action:
                    if not game_foreground():
                        raise RuntimeError('Moonlight game activity changed before input')
                    kind, *values = action
                    if kind == 'hold':
                        x, y, ms = values
                        values = [x, y, x, y, ms]
                    emit('input_start', checkpoint=name, action=action)
                    subprocess.run(adb + ['shell', 'input', 'swipe', *map(str, values)], check=True, timeout=8)
                    emit('input_end', checkpoint=name)
                    time.sleep(0.5)
            emit('complete', checkpoints=len(STAGES))
        except Exception as exc:
            emit('stopped', reason=str(exc))
            raise

if __name__ == '__main__':
    main()
