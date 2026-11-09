#!/usr/bin/env python3
"""Prepare the NXBridge 390 Moonlight experiment in a new Git worktree.

This never modifies the source checkout's working files. Submodules and Android
SDK/toolchain installation are deliberately separate, explicit steps.
"""
import argparse
from pathlib import Path
import re
import subprocess
import sys

PIN = 'b48494cb96bff23d8886c4775cc4f39a1075495d'
PATCHES = (
    'moonlight-native-baseline-318.patch',
    'moonlight-pad-layout.patch', 'moonlight-pad-layout-322.patch', 'moonlight-pad-layout-324.patch',
    'moonlight-pad-cover-325.patch', 'moonlight-library-cover-327.patch', 'moonlight-startup-loading-331.patch',
    'moonlight-layout-editor-350.patch', 'moonlight-layout-validation-351.patch', 'moonlight-safe-area-353.patch',
    'moonlight-layout-commit-354.patch', 'moonlight-input-lab-356.patch', 'moonlight-layout-recovery-357.patch',
    'moonlight-layout-history-359.patch', 'moonlight-layout-transfer-363.patch', 'moonlight-clean-version-365.patch',
    'moonlight-controller-toggle-366.patch', 'moonlight-clean-version-368.patch', 'moonlight-strict-layout-369.patch',
    'moonlight-safe-projection-370.patch', 'moonlight-lifecycle-input-371.patch', 'moonlight-repeat-guard-372.patch',
    'moonlight-held-guard-373.patch', 'moonlight-held-help-375.patch', 'moonlight-compact-preset-376.patch',
    'moonlight-compact-gap-377.patch', 'moonlight-face-group-378.patch', 'moonlight-face-resize-379.patch',
    'moonlight-stick-visual-380.patch', 'moonlight-stick-preview-381.patch', 'moonlight-stick-feedback-382.patch',
    'moonlight-haptic-guard-383.patch', 'moonlight-startup-input-384.patch', 'moonlight-neutral-rearm-386.patch',
    'moonlight-backup-export-387.patch', 'moonlight-clean-version-389.patch', 'moonlight-transfer-identity-390.patch',
)

def git(source, *args, capture=False):
    return subprocess.run(['git', '-C', str(source), *args], check=True, text=True,
                          stdout=subprocess.PIPE if capture else None)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', required=True, type=Path, help='existing Moonlight Git checkout at the pinned commit')
    parser.add_argument('--output', required=True, type=Path, help='new worktree path; must not exist')
    args = parser.parse_args()
    source = args.source.resolve(strict=True)
    output = args.output.absolute()
    if output.exists() or output.is_symlink():
        parser.error('output already exists; refusing to overwrite it')
    if not output.parent.is_dir():
        parser.error('output parent must already exist')
    try:
        revision = git(source, 'rev-parse', 'HEAD', capture=True).stdout.strip()
        if revision != PIN:
            parser.error('source HEAD does not match the pinned Moonlight revision')
        patches = [Path(__file__).resolve().parent / name for name in PATCHES]
        missing = [p.name for p in patches if not p.is_file()]
        if missing:
            parser.error('missing patches: ' + ', '.join(missing))
        git(source, 'worktree', 'add', '--detach', str(output), PIN)
        for i, patch in enumerate(patches, 1):
            print(f'[{i}/{len(patches)}] {patch.name}', flush=True)
            git(output, 'apply', '--check', str(patch))
            git(output, 'apply', str(patch))
        gradle = (output / 'app/build.gradle').read_text()
        if not re.search(r'^\s*versionCode\s*=\s*390\s*$', gradle, re.MULTILINE):
            raise RuntimeError('prepared version is not 390')
    except (OSError, subprocess.CalledProcessError, RuntimeError) as e:
        print(f'Preparation failed: {e}. If created, the partial worktree remains at {output}; inspect before removing it.', file=sys.stderr)
        return 1
    print(f'Prepared experimental client source at {output}. Initialize pinned submodules before building.')
    return 0

if __name__ == '__main__':
    sys.exit(main())
