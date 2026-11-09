#!/usr/bin/env python3
"""Check that the disconnected lab intercepts every ControllerHandler output.

Run against the patched Moonlight tree with --client PATH. This is a static
contract check, not proof of packet delivery or network isolation at runtime.
"""
import argparse
from pathlib import Path
import re

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--client', required=True, type=Path)
    root = parser.parse_args().client / 'app/src/main/java/com/limelight'
    handler = (root / 'binding/input/ControllerHandler.java').read_text()
    lab = (root / 'InputLabActivity.java').read_text()
    referenced = set(re.findall(r'\bconn\.(\w+)\s*\(', handler))
    marker = 'class LabConnection extends NvConnection'
    if marker not in lab:
        raise SystemExit('offline LabConnection is missing')
    overrides = set(re.findall(r'@Override\s+public\s+\w+\s+(\w+)\s*\(', lab[lab.index(marker):]))
    missing = referenced - overrides
    if missing:
        raise SystemExit('lab does not intercept controller output: ' + ', '.join(sorted(missing)))
    unsafe_direct = re.findall(r'\bMoonBridge\.(send\w+|start\w*|stop\w*|interrupt\w*)\s*\(', handler)
    if unsafe_direct:
        raise SystemExit('controller handler bypasses connection sink: ' + ', '.join(sorted(set(unsafe_direct))))
    print('PASS: lab overrides all ' + str(len(referenced)) + ' ControllerHandler connection methods')


if __name__ == "__main__":
    main()
