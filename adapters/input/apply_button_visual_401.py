#!/usr/bin/env python3
"""Apply independent button visual sizing to the NXBridge 398 client."""
import argparse
from pathlib import Path
import re
import subprocess

PIN = 'b48494cb96bff23d8886c4775cc4f39a1075495d'
parser = argparse.ArgumentParser()
parser.add_argument('--moonlight', type=Path, required=True)
args = parser.parse_args()
root = args.moonlight.resolve()
revision = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()
if revision != PIN:
    raise SystemExit('Moonlight revision mismatch')
if not re.search(r'^\s*versionCode\s*=\s*398\s*$', (root / 'app/build.gradle').read_text(), re.MULTILINE):
    raise SystemExit('Expected the NXBridge 398 client with earlier patches applied')
patch = Path(__file__).with_name('moonlight-button-visual-401.patch').resolve()
for options in (['--check'], []):
    subprocess.run(['git', '-C', str(root), 'apply', *options, str(patch)], check=True)
print('Applied NXBridge button visual sizing 401')
