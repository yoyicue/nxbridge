#!/usr/bin/env python3
"""Extend only the first-video-traffic deadline on the NXBridge 404 client."""
import argparse
from pathlib import Path
import re
import subprocess

PIN = 'b48494cb96bff23d8886c4775cc4f39a1075495d'
COMMON_PIN = '874ac9548f1bd6f095ef2b435c42cdde460e7821'
parser = argparse.ArgumentParser()
parser.add_argument('--moonlight', type=Path, required=True)
root = parser.parse_args().moonlight.resolve()
common = root / 'app/src/main/jni/moonlight-core/moonlight-common-c'

def revision(path):
    return subprocess.check_output(['git', '-C', str(path), 'rev-parse', 'HEAD'], text=True).strip()

if revision(root) != PIN or revision(common) != COMMON_PIN:
    raise SystemExit('Moonlight or moonlight-common-c revision mismatch')
if not re.search(r'^\s*versionCode\s*=\s*404\s*$', (root / 'app/build.gradle').read_text(), re.MULTILINE):
    raise SystemExit('Expected the NXBridge 404 client with earlier patches applied')
patches = [(root, Path(__file__).with_name('moonlight-first-video-405.patch').resolve()),
           (common, Path(__file__).with_name('moonlight-first-video-common-405.patch').resolve())]
for path, patch in patches:
    subprocess.run(['git', '-C', str(path), 'apply', '--check', str(patch)], check=True)
for path, patch in patches:
    subprocess.run(['git', '-C', str(path), 'apply', str(patch)], check=True)
print('Applied NXBridge first-video wait 405')
