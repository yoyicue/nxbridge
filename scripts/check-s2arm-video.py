#!/usr/bin/env python3
"""Exercise prepared S2Arm's real headless GPU path without game assets.

Requires a built engine, Xcode, ffmpeg and ffprobe. Never downloads dependencies.
The output directory must be new; logs and synthetic H.264 remain there.
"""
import argparse
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def build(engine, output, library=None, source=None):
    includes = ['graphics/nvn/gfx/nvn', 'graphics/nvn/gfx/nvn/include',
                'graphics/nvn/host', 'runtime/include',
                'graphics/nvn/third_party/vulkan-headers/include',
                'build/nvn/gen', 'build/nvn/tests', 'graphics/nvn/tests']
    weak = shlex.split(subprocess.check_output(
        [sys.executable, str(engine / 'tools/weak_link_decl.py'), 'flags', 'nvntest'], text=True))
    cmd = ['clang', '-std=gnu11', '-O1', '-g', '-fobjc-arc',
           '-mmacosx-version-min=15.0', '-Wno-unguarded-availability-new']
    cmd += ['-I' + str(engine / path) for path in includes]
    cmd += [str(source or ROOT / 'tests/s2arm_video.m'),
            str(library or engine / 'build/nvn/mac/libnvn.a'), '-o', str(output)]
    for framework in ['Metal', 'Foundation', 'QuartzCore', 'CoreGraphics', 'ImageIO',
                      'UniformTypeIdentifiers', 'VideoToolbox', 'CoreMedia', 'CoreVideo', 'IOSurface']:
        cmd += ['-framework', framework]
    return cmd + ['-lc++'] + weak


def decode(path):
    probe = json.loads(subprocess.check_output([
        'ffprobe', '-v', 'error', '-count_frames', '-show_entries',
        'stream=codec_name,width,height,nb_read_frames', '-of', 'json', str(path)], text=True))
    stream = probe['streams'][0]
    if (stream['codec_name'], stream['width'], stream['height'], stream['nb_read_frames']) != ('h264', 1280, 720, '14'):
        raise ValueError(f'unexpected encoded stream: {stream}')
    pixels = subprocess.check_output(['ffmpeg', '-v', 'error', '-i', str(path),
        '-vf', 'scale=1:1', '-pix_fmt', 'rgb24', '-f', 'rawvideo', '-'])
    expected = (51, 102, 204)
    if len(pixels) != 14 * 3 or any(abs(pixel - expected[i % 3]) > 12 for i, pixel in enumerate(pixels)):
        raise ValueError('decoded pixels do not match the synthetic source')
    return {'probe': probe, 'rgb_all_frames': [list(pixels[i:i+3]) for i in range(0, len(pixels), 3)],
            'expected_rgb': expected, 'tolerance': 12}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--engine', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    engine = args.engine.resolve(strict=True)
    output = args.output.absolute()
    output.mkdir(parents=False, exist_ok=False)
    executable = output / 's2arm-video'
    with (output / 'build.log').open('w') as log:
        subprocess.run(['make', '-C', str(engine / 'graphics/nvn'), 'widen-stubs'],
                       stdout=log, stderr=subprocess.STDOUT, check=True, timeout=120)
        subprocess.run(build(engine, executable), stdout=log, stderr=subprocess.STDOUT, check=True, timeout=120)
    # No inherited Mach ports, probe paths, game profiles or rendering overrides.
    clean = {key: value for key, value in os.environ.items()
             if not key.startswith(('NXB_', 'NXBRIDGE_', 'DM_', 'S2A_', 'NXGAL_', 'S2ARM_'))}
    for backend, pressure in (('mtl4', False), ('mtl3', False), ('mtl4', True), ('mtl3', True)):
        name = backend + ('-pressure' if pressure else '')
        folder = output / name; folder.mkdir()
        env = dict(clean)
        if backend == 'mtl3': env['NXGAL_BE'] = 'mtl3'
        with (folder / 'run.log').open('w') as log:
            subprocess.run([str(executable), str(folder / 'out.h264')] + (['--pressure'] if pressure else []), env=env, cwd=folder,
                           stdout=log, stderr=subprocess.STDOUT, check=True, timeout=20)
        log = (folder / 'run.log').read_text()
        required = ['direct backend: Metal ' + ('4' if backend == 'mtl4' else '3'),
                    f'claims=14 encoded=14 dropped={int(pressure)} failed=0 inflight=0', 'PASS: headless S2Arm']
        if not all(value in log for value in required):
            raise ValueError(f'{name} did not report the expected backend and successful encoder drain')
        (folder / 'decoded.json').write_text(json.dumps(decode(folder / 'out.h264'), indent=2) + '\n')
        print(f'{name}: 14 frames encoded and decoded; {int(pressure)} deliberate drop; producer-event dependency checked')
    print('Synthetic path only; game compatibility and game FPS are untested.')


if __name__ == '__main__':
    main()
