#!/usr/bin/env python3
"""Build a disposable snapshot of the Git candidate files, using existing toolchains."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def main():
    listing = subprocess.check_output(['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z'], cwd=ROOT)
    names = sorted(set(os.fsdecode(x) for x in listing.split(b'\0') if x))
    with tempfile.TemporaryDirectory(prefix='nxbridge-source-') as temp:
        snapshot = Path(temp) / 'source'
        snapshot.mkdir()
        for name in names:
            source = ROOT / name
            if source.is_symlink() or not source.is_file():
                raise ValueError(f'cannot snapshot non-regular file: {name}')
            dest = snapshot / name
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, dest)
        print(f'Isolated source snapshot: {len(names)} files; existing Rust and native dependencies are reused.', flush=True)
        command = ['cargo', 'build', '--locked', '--workspace']
        if os.environ.get('NXB_CARGO_OFFLINE') == '1':
            command.append('--offline')
        subprocess.run(command, cwd=snapshot, check=True)
        subprocess.run([sys.executable, 'scripts/synthetic-selfcheck.py'], cwd=snapshot, check=True)
    print('PASS: isolated candidate snapshot build and local synthetic self-check.')
    print('This is not a clean OS installation or an end-to-end GameStream test.')


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as exc:
        print(f'Isolated build failed: {exc}', file=sys.stderr)
        sys.exit(1)
