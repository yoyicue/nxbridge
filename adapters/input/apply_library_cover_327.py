#!/usr/bin/env python3
"""Apply library cover refresh to the existing NXBridge 325 Moonlight tree."""
import argparse
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument('--moonlight', type=Path, required=True)
args = parser.parse_args()
root = args.moonlight.resolve()
revision = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()
if revision != 'b48494cb96bff23d8886c4775cc4f39a1075495d':
    raise SystemExit('Moonlight revision mismatch')
patch = Path(__file__).with_name('moonlight-library-cover-327.patch')
for options in (['--check'], []):
    subprocess.run(['git', '-C', str(root), 'apply', *options, str(patch)], check=True)
print('Applied NXBridge library cover refresh 327')
