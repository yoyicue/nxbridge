#!/usr/bin/env python3
"""Launch isolation checks using synthetic files and a harmless child process."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / 'adapters/s2arm/launch.py'
spec = importlib.util.spec_from_file_location('s2arm_launch', SCRIPT)
launch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(launch)


class LaunchTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.engine, self.game, self.save, self.work = [self.root / name for name in ('engine', 'game', 'save', 'work')]
        for path in (self.engine, self.game, self.save, self.work): path.mkdir()
        (self.engine / 'build').mkdir()
        (self.engine / 'tools').mkdir()
        # This fixture only checks fail-closed propagation; it does not claim
        # to validate a real shader library or replace the upstream check.
        (self.engine / 'tools/mac_chain_check.py').write_text('import sys\nsys.exit(0)\n')
        (self.engine / 'build/s2arm-host').write_text('#!/bin/sh\nexec /bin/sleep 15\n')
        (self.engine / 'build/s2arm-host').chmod(0o755)
        (self.engine / 'source').write_text('synthetic fixture')
        self.manifest = {'schema_version': 1, 'files': {'source': hashlib.sha256(b'synthetic fixture').hexdigest()}}
        self.write_manifest()
        for name in ('main.patched.bin', 'main.npdm', 'romfs'): (self.game / name).write_bytes(b'fixture')
        self.env = {
            'DM_SAVE': str(self.save), 'DM_TEMP': str(self.work),
            'NXB_SESSION_ID': '00112233445566778899aabbccddeeff', 'NXB_VIDEO_GENERATION': '1',
            'NXB_MACH_MEDIA': '1', 'NXBRIDGE_INPUT_TOKEN': 'synthetic-test-token',
            'NXBRIDGE_INPUT_PORT': '40000', 'PATH': os.environ['PATH'],
        }

    def write_manifest(self):
        (self.engine / '.nxbridge-s2arm.json').write_text(json.dumps(self.manifest))

    def tearDown(self): self.tmp.cleanup()
    def plan(self, env=None, **changes):
        args = dict(engine=self.engine, game=self.game, save=self.save, work=self.work, inherited=env or self.env)
        args.update(changes)
        return launch.plan(**args)

    def command(self): return [sys.executable, str(SCRIPT), '--engine', str(self.engine), str(self.game)]

    def test_private_paths_and_input_identity(self):
        env = {**self.env, 'S2A_SAVE_DIR': '/another-player', 'S2A_AUTOPLAY': '1',
               'NXGAL_SHADER_CACHE_DIR': '/shared', 'DM_CHEATS': '1', 'DM_WINDOW': '1'}
        _, _, _, run, actual = self.plan(env)
        self.assertEqual(actual['S2A_SAVE_DIR'], '')
        self.assertEqual(actual['S2A_RYU_DATA'], str(self.save.resolve() / 'ryu'))
        self.assertEqual(actual['S2A_GAME_DATA'], str(self.game.resolve()))
        self.assertEqual(actual['S2A_AUTOPLAY'], '0')
        self.assertEqual(actual['S2A_DOCS'], str(run))
        self.assertEqual(actual['NXGAL_SHADER_CACHE_DIR'], str(run / 'gal'))
        self.assertNotIn('DM_CHEATS', actual)
        self.assertNotIn('DM_WINDOW', actual)
        self.assertEqual(actual['NXBRIDGE_INPUT_TOKEN'], env['NXBRIDGE_INPUT_TOKEN'])
        self.assertEqual(actual['NXB_SESSION_ID'], env['NXB_SESSION_ID'])

    def test_overlap_and_symlink_alias_are_rejected(self):
        alias = self.root / 'alias'; alias.symlink_to(self.save)
        for path in (self.save, alias, self.root):
            with self.assertRaises(ValueError): self.plan(work=path)

    def test_session_identity_required(self):
        for changes in ({'NXB_SESSION_ID': ''}, {'NXB_SESSION_ID': '0' * 32},
                        {'NXB_VIDEO_GENERATION': '0'}, {'NXB_VIDEO_GENERATION': str(2**64)},
                        {'NXB_VIDEO_GENERATION': '-1'}):
            with self.assertRaises(ValueError): self.plan({**self.env, **changes})

    def test_persistent_state_is_shared_only_across_same_players_generations(self):
        state = self.save / 'ryu'; state.mkdir()
        (state / 'save-fixture').write_bytes(b'persistent synthetic save')
        first = self.plan()[-1]
        second = self.plan({**self.env, 'NXB_VIDEO_GENERATION': '2'})[-1]
        self.assertEqual(first['S2A_RYU_DATA'], second['S2A_RYU_DATA'])
        self.assertNotEqual(first['S2A_DOCS'], second['S2A_DOCS'])
        self.assertEqual((Path(second['S2A_RYU_DATA']) / 'save-fixture').read_bytes(), b'persistent synthetic save')
        other = self.root / 'other-player'; other.mkdir()
        self.assertNotEqual(first['S2A_RYU_DATA'], self.plan(save=other)[-1]['S2A_RYU_DATA'])

    def test_persistent_state_cannot_alias_another_player(self):
        (self.save / 'ryu').symlink_to(self.work)
        with self.assertRaisesRegex(ValueError, 'persistent state'): self.plan()

    def test_modified_prepared_source_is_rejected(self):
        (self.engine / 'source').write_text('changed')
        with self.assertRaisesRegex(ValueError, 'source changed'): self.plan()

    def test_manifest_cannot_escape_engine(self):
        self.manifest['files'] = {'../outside': '0' * 64}; self.write_manifest()
        with self.assertRaisesRegex(ValueError, 'manifest path'): self.plan()

    def test_unbuilt_game_is_rejected(self):
        (self.game / 'main.patched.bin').unlink()
        with self.assertRaisesRegex(ValueError, 'build for macOS first'): self.plan()

    def test_dry_run_has_no_write_or_token_disclosure(self):
        result = subprocess.run(self.command() + ['--dry-run'], env=self.env, capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn(self.env['NXBRIDGE_INPUT_TOKEN'], result.stdout + result.stderr)
        self.assertEqual(list(self.save.iterdir()), [])
        self.assertEqual(list(self.work.iterdir()), [])

    def test_upstream_chain_rejection_leaves_save_and_work_untouched(self):
        (self.engine / 'tools/mac_chain_check.py').write_text('import sys\nsys.exit(1)\n')
        result = subprocess.run(self.command(), env=self.env, capture_output=True, text=True, timeout=5)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(list(self.save.iterdir()), [])
        self.assertEqual(list(self.work.iterdir()), [])

    def test_preflight_sees_filtered_environment(self):
        (self.engine / 'tools/mac_chain_check.py').write_text(
            'import os, sys\nassert "S2A_ALLOW_CHAIN_MISMATCH" not in os.environ\n'
            'assert len(sys.argv) == 2\nsys.exit(0)\n')
        result = subprocess.run(self.command() + ['--dry-run'],
            env={**self.env, 'S2A_ALLOW_CHAIN_MISMATCH': '1'}, capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(list(self.save.iterdir()), [])
        self.assertEqual(list(self.work.iterdir()), [])

    @unittest.skipUnless(sys.platform == 'darwin', 'Mach handoff requires macOS')
    def test_mach_media_endpoint_survives_preflight_and_exec(self):
        probe = self.root / 'mach-handoff'
        subprocess.run(['clang', '-O2', '-Wall', '-Wextra', '-Werror',
            str(ROOT / 'tests/s2arm_mach_handoff.c'), '-o', str(probe)], check=True)
        shutil.copy2(probe, self.engine / 'build/s2arm-host')
        for generation, sandboxed in enumerate((False, True), 1):
            with self.subTest(sandboxed=sandboxed):
                command = self.command()
                if sandboxed:
                    command = ['/usr/bin/sandbox-exec', '-p', '(version 1)(allow default)', *command]
                result = subprocess.run([str(probe), '--spawn', *command],
                    env={**self.env, 'NXB_VIDEO_GENERATION': str(generation)},
                    capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn('delivered the host message', result.stdout)

    def test_save_lock_survives_exec_and_releases_on_exit(self):
        first = subprocess.Popen(self.command(), env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
        try:
            for _ in range(100):
                if list(self.work.iterdir()): break
                if first.poll() is not None: self.fail(first.communicate()[1].decode())
                time.sleep(.02)
            else: self.fail('synthetic launcher did not start')
            other_env = {**self.env, 'NXB_VIDEO_GENERATION': '2'}
            second = subprocess.run(self.command(), env=other_env, capture_output=True, text=True, timeout=5)
            self.assertNotEqual(second.returncode, 0)
            self.assertEqual(len(list(self.work.iterdir())), 1)
        finally:
            first.terminate(); first.communicate(timeout=3)
        # Same generation cannot overwrite its previous diagnostic directory.
        stale = subprocess.run(self.command(), env=self.env, capture_output=True, text=True, timeout=5)
        self.assertNotEqual(stale.returncode, 0)
        self.assertIn('File exists', stale.stderr)
        third = subprocess.Popen(self.command(), env=other_env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
        try:
            for _ in range(100):
                if len(list(self.work.iterdir())) == 2: break
                time.sleep(.02)
            else: self.fail('save lock was not released')
        finally:
            third.terminate(); third.communicate(timeout=3)


if __name__ == '__main__':
    unittest.main()
