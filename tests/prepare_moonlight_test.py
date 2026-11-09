#!/usr/bin/env python3
"""Safety checks for the experimental Moonlight source preparation entry point."""
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / 'adapters/input/prepare_moonlight_390.py'

class PrepareMoonlightTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.repo = self.root / 'upstream'
        self.repo.mkdir()
        subprocess.run(['git', 'init', '-q', str(self.repo)], check=True)
        (self.repo / 'unchanged').write_text('keep me')
        subprocess.run(['git', '-C', str(self.repo), 'add', 'unchanged'], check=True)
        subprocess.run(['git', '-C', str(self.repo), '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid',
                        'commit', '-qm', 'fixture'], check=True)

    def tearDown(self):
        self.tmp.cleanup()

    def run_prepare(self, output):
        return subprocess.run(['python3', str(SCRIPT), '--source', str(self.repo), '--output', str(output)],
                              capture_output=True, text=True)

    def test_existing_output_is_never_overwritten(self):
        output = self.root / 'existing'; output.mkdir()
        sentinel = output / 'sentinel'; sentinel.write_text('retain')
        result = self.run_prepare(output)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(sentinel.read_text(), 'retain')
        self.assertIn('output already exists', result.stderr)
        self.assertEqual((self.repo / 'unchanged').read_text(), 'keep me')

    def test_wrong_upstream_revision_does_not_create_output(self):
        output = self.root / 'new-tree'
        result = self.run_prepare(output)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(output.exists())
        self.assertIn('does not match', result.stderr)
        self.assertEqual((self.repo / 'unchanged').read_text(), 'keep me')

if __name__ == '__main__':
    unittest.main()
