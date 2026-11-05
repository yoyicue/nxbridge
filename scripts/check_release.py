#!/usr/bin/env python3
"""Heuristic checks for files Git would include; this is not a license audit."""
import argparse
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
FORBIDDEN_SUFFIXES = {'.apk', '.ipa', '.nsp', '.xci', '.nsz', '.xcz', '.nca',
                      '.nso', '.rpx', '.wua', '.wud', '.wux', '.sav', '.keys',
                      '.key', '.p12', '.pfx', '.mobileprovision'}
PRIVATE_PATH = re.compile(rb'/' + rb'(?:Users|Volumes)/[A-Za-z0-9._-]+')
PRIVATE_KEY = re.compile(rb'-----BEGIN (?:[A-Z0-9 ]+ )?PRIVATE KEY-----')
MAX_FILE_BYTES = 2 * 1024 * 1024


def inspect_file(root: Path, name: str) -> list[str]:
    path = root / name
    issues = []
    if path.is_symlink():
        return ['symlink requires manual review']
    if path.suffix.lower() in FORBIDDEN_SUFFIXES:
        issues.append('prohibited game, credential or binary extension')
    if not path.is_file():
        return issues + ['candidate is not a regular file']
    if path.stat().st_size > MAX_FILE_BYTES:
        return issues + ['file exceeds 2 MiB and requires manual review']
    data = path.read_bytes()
    if b'\0' in data:
        issues.append('binary file requires manual review')
    if PRIVATE_PATH.search(data):
        issues.append('possible machine-specific path')
    if PRIVATE_KEY.search(data):
        issues.append('possible private key')
    return issues


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--content-only', action='store_true',
                        help='scan contents while the project license is undecided')
    args = parser.parse_args()
    try:
        listing = subprocess.check_output(
            ['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z'], cwd=ROOT)
        names = sorted(set(os.fsdecode(x) for x in listing.split(b'\0') if x))
    except (OSError, subprocess.CalledProcessError) as exc:
        print(f'Cannot enumerate Git candidates: {exc}', file=sys.stderr)
        return 2
    findings = []
    for name in names:
        findings.extend((name, issue) for issue in inspect_file(ROOT, name))
    if not args.content_only and not any(name in names for name in ('LICENSE', 'LICENSE.md', 'LICENSE.txt')):
        findings.append(('<root>', 'project license has not been selected'))
    for name, issue in findings:
        print(f'{name}: {issue}')
    print(f'Checked {len(names)} Git candidates; {len(findings)} finding(s).')
    print('Heuristic only: review source rights, dependencies, secrets and actual release artifacts manually.')
    return 1 if findings else 0


if __name__ == '__main__':
    raise SystemExit(main())
