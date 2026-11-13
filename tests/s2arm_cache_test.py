#!/usr/bin/env python3
"""Persistent cache isolation and crash recovery with synthetic artifacts."""
import fcntl
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'adapters/s2arm'))
import s2arm_cache as cache


class CacheTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.engine, self.game, self.work = [self.root / p for p in ('engine', 'game', 'work')]
        for p in (self.engine / 'build', self.game, self.work): p.mkdir(parents=True)
        (self.engine / 'build/s2arm-host').write_bytes(b'host')
        (self.game / 'main.patched.bin').write_bytes(b'game-a')
        (self.game / 'main.npdm').write_bytes(b'npdm-a')
        self.manifest = {'schema_version': 1, 'upstream_revision': 'revision', 'files': {'source': 'hash'}}
        self.fds = []

    def tearDown(self):
        for fd in self.fds: os.close(fd)
        self.tmp.cleanup()

    def identity(self): return cache.identity(self.engine, self.game, self.manifest)

    def acquire(self, identity=None):
        path, fd = cache.acquire(self.work, identity or self.identity())
        self.fds.append(fd)
        return path

    def test_identity_changes_with_game_code_metadata_engine_and_os(self):
        original = self.identity()
        for path in (self.game / 'main.patched.bin', self.game / 'main.npdm', self.engine / 'build/s2arm-host'):
            data = path.read_bytes(); path.write_bytes(data + b'changed')
            self.assertNotEqual(cache.key(original), cache.key(self.identity()))
            path.write_bytes(data)
        self.manifest['files']['source'] = 'new hash'
        self.assertNotEqual(cache.key(original), cache.key(self.identity()))
        other_os = {**original, 'platform': {'release': 'other'}}
        self.assertNotEqual(cache.key(original), cache.key(other_os))

    def test_secondary_module_and_shader_index_invalidate(self):
        original = self.identity()
        (self.game / 'sdk.patched.bin').write_bytes(b'sdk')
        self.assertNotEqual(cache.key(original), cache.key(self.identity()))
        original = self.identity()
        directory = self.game / 'shaders/direct/b-chain'; directory.mkdir(parents=True)
        (directory / 'lib.idx').write_bytes(b'changed library index')
        self.assertNotEqual(cache.key(original), cache.key(self.identity()))

    def test_atomic_entry_and_restart_reuse(self):
        identity = self.identity(); path = self.acquire(identity)
        self.assertEqual(json.loads((path / 'identity.json').read_text()), identity)
        self.assertTrue((path / 'shaders').is_dir())
        self.assertTrue((path / 'gal').is_dir())
        (path / 'shaders/test.metal').write_bytes(b'cache fixture')
        os.close(self.fds.pop())
        self.assertEqual(self.acquire(identity), path)
        self.assertEqual((path / 'shaders/test.metal').read_bytes(), b'cache fixture')
        self.assertFalse(any(p.name.startswith('.staging-') for p in path.parent.iterdir()))

    def test_busy_entry_rejected_but_other_build_can_run(self):
        path = self.acquire()
        with self.assertRaises(BlockingIOError): self.acquire()
        self.assertTrue(os.get_inheritable(self.fds[0]))
        self.assertNotEqual(self.acquire({**self.identity(), 'schema_version': 999}), path)

    def test_identity_corruption_is_not_silently_overwritten(self):
        path = self.acquire(); os.close(self.fds.pop())
        (path / 'identity.json').write_text('{}')
        with self.assertRaisesRegex(ValueError, 'cache identity'): self.acquire()
        self.assertEqual((path / 'identity.json').read_text(), '{}')

    def test_symlink_root_entry_and_descendant_are_rejected(self):
        outside = self.root / 'outside'; outside.mkdir()
        root = self.work / '.s2arm-cache'
        root.symlink_to(outside)
        with self.assertRaisesRegex(ValueError, 'symlink'): self.acquire()
        root.unlink(); root.mkdir()
        target = root / cache.key(self.identity()); target.symlink_to(outside)
        with self.assertRaisesRegex(ValueError, 'symlink'): self.acquire()
        target.unlink()
        path = self.acquire(); os.close(self.fds.pop())
        (path / 'shaders/escape.metal').symlink_to(outside / 'important')
        with self.assertRaisesRegex(ValueError, 'symlink'): self.acquire()
        self.assertEqual(list(outside.iterdir()), [])

    def test_lock_symlink_is_rejected(self):
        root = self.work / '.s2arm-cache'; root.mkdir()
        outside = self.root / 'important'; outside.write_bytes(b'retain')
        (root / (cache.key(self.identity()) + '.lock')).symlink_to(outside)
        with self.assertRaises(OSError): self.acquire()
        self.assertEqual(outside.read_bytes(), b'retain')

    def test_crashed_manifest_tail_is_removed_before_append(self):
        path = self.acquire(); os.close(self.fds.pop())
        manifest = path / 'pso_user.tsv'
        manifest.write_bytes(b'# fixture\nS\tcomplete\t0\nR\tincomplete')
        self.acquire()
        self.assertEqual(manifest.read_bytes(), b'# fixture\nS\tcomplete\t0\n')


if __name__ == '__main__': unittest.main()
