#!/usr/bin/env python3
"""Prepare an isolated, pinned S2Arm checkout with NXBridge's headless adapters.

This copies committed source into a new checkout. It does not install tools,
build games, start services, or modify the source working tree.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
LOCK = ROOT / 'config/s2arm-experiment-lock.json'
PIN = json.loads(LOCK.read_text())['revision']
FILES = (
    ('adapters/input/nxbridge_input.h', 'runtime/include/nxbridge_input.h'),
    ('adapters/input/nxbridge_input.c', 'runtime/src/nxbridge_input.c'),
    ('adapters/s2arm/input.c', 'runtime/src/nxbridge_remote.c'),
    ('adapters/nxnative/pcm_queue.h', 'runtime/include/pcm_queue.h'),
    ('adapters/nxnative/pcm_queue.c', 'runtime/src/nxbridge_pcm_queue.c'),
    ('adapters/nxnative/pcm_probe.c', 'runtime/src/nxbridge_pcm_probe.c'),
    ('adapters/nxnative/mach_media.h', 'runtime/include/mach_media.h'),
    ('adapters/nxnative/mach_media.h', 'graphics/nvn/gfx/nvn/mach_media.h'),
    ('adapters/nxnative/video_probe.h', 'graphics/nvn/gfx/nvn/nxbridge_video_probe.h'),
    ('adapters/nxnative/video_probe.m', 'graphics/nvn/gfx/nvn/nxbridge_video_probe.m'),
    ('adapters/s2arm/present.inc', 'graphics/nvn/gfx/nvn/nxbridge_present.inc'),
    ('adapters/s2arm/gpu_timing.h', 'graphics/nvn/gfx/nvn/nxbridge_gpu_timing.h'),
    ('adapters/s2arm/AudioDriver.cs', 'platform/apple/cs/audio/NXBridgeAudioDriver.cs'),
    ('adapters/s2arm/launch.py', 'nxbridge/launch.py'),
    ('adapters/s2arm/headless.mk', 'nxbridge/build.mk'),
)


def git(root, *args, capture=False):
    return subprocess.run(['git', '-C', str(root), *args], check=True, text=True,
                          stdout=subprocess.PIPE if capture else None)


def adapter_bytes(source):
    data = (ROOT / source).read_bytes()
    if source == 'adapters/nxnative/video_probe.m':
        data = data.replace(b'#import "video_probe.h"', b'#import "nxbridge_video_probe.h"')
    return data


def manifest(root):
    paths = [target for _, target in FILES]
    paths += git(root, 'diff', '--name-only', capture=True).stdout.splitlines()
    return {
        'schema_version': 1,
        'upstream_revision': PIN,
        'patch_sha256': hashlib.sha256((HERE / 'integration.patch').read_bytes()).hexdigest(),
        'files': {p: hashlib.sha256((root / p).read_bytes()).hexdigest() for p in sorted(set(paths))},
    }


def prepare(source, output):
    source = source.resolve(strict=True)
    output = output.absolute()
    if output.exists() or output.is_symlink():
        raise ValueError('output already exists; refusing to overwrite it')
    if not output.parent.is_dir():
        raise ValueError('output parent must already exist')
    if git(source, 'rev-parse', 'HEAD', capture=True).stdout.strip() != PIN:
        raise ValueError('source HEAD does not match the pinned S2Arm revision')
    # Check local input availability before creating anything. Cloning reads
    # Git objects, so uncommitted upstream work is deliberately not imported.
    for path, _ in FILES:
        adapter_bytes(path)
    patch = HERE / 'integration.patch'
    if not patch.is_file():
        raise ValueError('missing integration.patch')
    subprocess.run(['git', 'clone', '--no-hardlinks', '--no-checkout', str(source), str(output)], check=True)
    git(output, 'checkout', '--detach', PIN)
    git(output, 'apply', '--check', str(patch))
    for source_path, target in FILES:
        dest = output / target
        if dest.exists() or dest.is_symlink():
            raise ValueError(f'refusing to overwrite {target}')
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(adapter_bytes(source_path))
    (output / 'nxbridge/launch.py').chmod(0o755)
    git(output, 'apply', str(patch))
    (output / '.nxbridge-s2arm.json').write_text(json.dumps(manifest(output), indent=2) + '\n')
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    try:
        output = prepare(args.source, args.output)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f'Preparation failed: {error}. Any partial output is retained for inspection.', file=sys.stderr)
        return 1
    print(f'Prepared pinned headless source at {output}; build with make host S2A_CS=1.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
