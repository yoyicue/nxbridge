#!/usr/bin/env python3
"""Keep floating-stick visuals visible outside their touch zones."""
import argparse
from pathlib import Path
import re
import subprocess

PIN = 'b48494cb96bff23d8886c4775cc4f39a1075495d'
parser = argparse.ArgumentParser()
parser.add_argument('--moonlight', type=Path, required=True)
root = parser.parse_args().moonlight.resolve()
revision = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()
if revision != PIN:
    raise SystemExit('Moonlight revision mismatch')
if not re.search(r'^\s*versionCode\s*=\s*405\s*$', (root / 'app/build.gradle').read_text(), re.MULTILINE):
    raise SystemExit('Expected the NXBridge 405 client with earlier patches applied')
patch = Path(__file__).with_name('moonlight-stick-edge-406.patch').resolve()
for options in (['--check'], []):
    subprocess.run(['git', '-C', str(root), 'apply', *options, str(patch)], check=True)
print('Applied NXBridge floating-stick edge rendering 406')
