#!/usr/bin/env python3
"""Fail-closed preparation checks; no upstream download or game required."""
from pathlib import Path
import subprocess
import json
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / 'adapters/s2arm/prepare.py'


class PrepareTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.source = self.root / 'source'
        subprocess.run(['git', 'init', '-q', str(self.source)], check=True)
        (self.source / 'sentinel').write_text('committed')
        subprocess.run(['git', '-C', str(self.source), 'add', '.'], check=True)
        subprocess.run(['git', '-C', str(self.source), '-c', 'user.name=Test',
                        '-c', 'user.email=test@example.invalid', 'commit', '-qm', 'fixture'], check=True)
        (self.source / 'sentinel').write_text('uncommitted work')

    def tearDown(self):
        self.assertEqual((self.source / 'sentinel').read_text(), 'uncommitted work')
        self.tmp.cleanup()

    def run_prepare(self, output):
        return subprocess.run(['python3', str(SCRIPT), '--source', str(self.source), '--output', str(output)],
                              capture_output=True, text=True, timeout=15)

    def test_existing_directory_is_not_overwritten(self):
        output = self.root / 'existing'; output.mkdir()
        (output / 'keep').write_text('retain')
        result = self.run_prepare(output)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('output already exists', result.stderr)
        self.assertEqual((output / 'keep').read_text(), 'retain')

    def test_dangling_symlink_is_not_followed(self):
        output = self.root / 'link'; output.symlink_to(self.root / 'missing')
        result = self.run_prepare(output)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('output already exists', result.stderr)
        self.assertTrue(output.is_symlink())
        self.assertFalse((self.root / 'missing').exists())

    def test_candidate_revision_still_requires_exact_source(self):
        lock = json.loads((ROOT / 'config/s2arm-experiment-lock.json').read_text())
        revision = next(iter(lock['candidate_revisions']))
        output = self.root / 'candidate'
        result = subprocess.run(['python3', str(SCRIPT), '--source', str(self.source),
                                 '--output', str(output), '--revision', revision],
                                capture_output=True, text=True, timeout=15)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('does not match', result.stderr)
        self.assertFalse(output.exists())

    def test_unlisted_revision_is_rejected(self):
        output = self.root / 'unlisted'
        revision = subprocess.check_output(['git', '-C', str(self.source), 'rev-parse', 'HEAD'], text=True).strip()
        result = subprocess.run(['python3', str(SCRIPT), '--source', str(self.source),
                                 '--output', str(output), '--revision', revision],
                                capture_output=True, text=True, timeout=15)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(output.exists())

    def test_wrong_commit_does_not_create_output(self):
        output = self.root / 'new'
        result = self.run_prepare(output)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('does not match', result.stderr)
        self.assertFalse(output.exists())


if __name__ == '__main__':
    unittest.main()
