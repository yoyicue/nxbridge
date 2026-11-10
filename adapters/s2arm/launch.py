#!/usr/bin/env python3
"""Launch a prepared S2Arm game with NXBridge's per-player save/work directories.

GameStream can use ENGINE/nxbridge/launch.py as its host and a prepared Mac game
root as its game_dir. DM_SAVE and DM_TEMP keep the existing catalog contract.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import runpy
import sys


def directory(value, label):
    if not value:
        raise ValueError(f'{label} is required')
    path = Path(value)
    if not path.is_absolute() or not path.is_dir():
        raise ValueError(f'{label} must be an existing absolute directory')
    return path.resolve()


def overlaps(a, b):
    return a == b or a in b.parents or b in a.parents


def plan(engine, game, save, work, inherited):
    engine = directory(engine, 'engine')
    game = directory(game, 'game root')
    save = directory(save, 'DM_SAVE')
    work = directory(work, 'DM_TEMP')
    if any(overlaps(a, b) for a, b in ((save, work), (save, game), (work, game), (save, engine), (work, engine))):
        raise ValueError('save, work, game and engine paths must not overlap writable storage')
    state = save / 'ryu'
    if state.is_symlink() or (state.exists() and not state.is_dir()):
        raise ValueError('persistent state must be a directory inside DM_SAVE')
    if not (engine / '.nxbridge-s2arm.json').is_file():
        raise ValueError('engine has no NXBridge preparation manifest')
    manifest = json.loads((engine / '.nxbridge-s2arm.json').read_text())
    if manifest.get('schema_version') != 1 or not manifest.get('files'):
        raise ValueError('invalid preparation manifest')
    for relative, digest in manifest['files'].items():
        relative_path = Path(relative)
        if relative_path.is_absolute() or '..' in relative_path.parts:
            raise ValueError('invalid preparation manifest path')
        if hashlib.sha256((engine / relative_path).read_bytes()).hexdigest() != digest:
            raise ValueError(f'prepared source changed: {relative}')
    host = engine / 'build/s2arm-host'
    if not host.is_file() or not os.access(host, os.X_OK):
        raise ValueError('prepared headless host has not been built')
    for name in ('main.patched.bin', 'main.npdm'):
        if not (game / name).is_file():
            raise ValueError(f'game root is missing {name}; build for macOS first')
    if not (game / 'romfs').exists():
        raise ValueError('game root is missing romfs')
    session = inherited.get('NXB_SESSION_ID', '')
    generation = inherited.get('NXB_VIDEO_GENERATION', '')
    if not re.fullmatch(r'[0-9a-fA-F]{32}', session) or int(session, 16) == 0:
        raise ValueError('a nonzero NXB_SESSION_ID is required')
    if not re.fullmatch(r'[0-9]+', generation) or not 0 < int(generation) < 2**64:
        raise ValueError('NXB_VIDEO_GENERATION must be a positive u64')
    run = work / f's2arm-{session.lower()}-{int(generation)}'
    # A build/profile never inherits another game's data, diagnostics, autoplay,
    # cheats, renderer cache or platform settings from the service process.
    env = {key: value for key, value in inherited.items()
           if not key.startswith(('DM_', 'S2A_', 'NXGAL_', 'S2ARM_'))}
    env.update({
        # SAVE_DIR imports a seed destructively; RYU_DATA owns live saves and
        # profiles. Keep it across generations and explicitly disable seeding
        # (an empty value also prevents the upstream .env from enabling it).
        'S2A_SAVE_DIR': '', 'S2A_GAME_DATA': str(game),
        'S2A_DOCS': str(run), 'S2A_RYU_DATA': str(state),
        'NXGAL_SHADER_CACHE_DIR': str(run / 'gal'),
        'S2A_AUTOPLAY': '0', 'S2A_AUDIO_OFF': '0',
        'S2A_FRAMES_TSV': str(run / 'frames.tsv'),
        'S2A_AUTOPLAY_OUT': str(run), 'S2A_RUN_ROOT': str(run / 'runtime'),
        'S2A_MACRO_DUMP': str(run / 'macro'),
    })
    return host, game, save, run, env


def check_chain(script, game, env):
    # On macOS, spawning a Python preflight child can clear inherited registered
    # Mach ports in the launcher. Run the upstream read-only CLI in this process
    # so the worker's media send right survives the final exec into the host.
    original_env, original_argv = dict(os.environ), sys.argv
    try:
        os.environ.clear()
        os.environ.update(env)
        sys.argv = [str(script), str(game)]
        try:
            runpy.run_path(str(script), run_name='__main__')
        except SystemExit as result:
            if result.code not in (None, 0):
                raise ValueError(f'upstream shader-chain check failed: {result.code}') from result
    finally:
        sys.argv = original_argv
        os.environ.clear()
        os.environ.update(original_env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('game_root')
    parser.add_argument('--engine', default=str(Path(__file__).resolve().parents[1]))
    parser.add_argument('--dry-run', action='store_true')
    args = parser.parse_args()
    try:
        host, game, save, run, env = plan(args.engine, args.game_root,
                                        os.environ.get('DM_SAVE'), os.environ.get('DM_TEMP'), os.environ)
        # Use the upstream read-only compatibility check before acquiring a
        # save lock or creating a session. The mismatch override was filtered.
        check_chain(host.parents[1] / 'tools/mac_chain_check.py', game, env)
        if args.dry_run:
            # Do not print inherited input tokens or credentials.
            print(json.dumps({'host': str(host), 'game': str(game), 'save': str(save), 'run': str(run)}, indent=2))
            return 0
        # Preserve this descriptor across exec. The child process owns the save
        # lock until exit; a second launch fails instead of sharing a writer.
        lock_fd = os.open(save / '.nxbridge-s2arm.lock', os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
        fcntl.flock(lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        os.set_inheritable(lock_fd, True)
        run.mkdir(mode=0o700, exist_ok=False)
        (save / 'ryu').mkdir(mode=0o700, exist_ok=True)
        for child in ('gal', 'macro', 'runtime'):
            (run / child).mkdir(mode=0o700)
        os.chdir(run)
        os.execve(host, [str(host), str(game)], env)
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f'S2Arm launch refused: {error}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
