import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('check_release', Path(__file__).resolve().parents[1] / 'scripts' / 'check_release.py')
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class ReleaseContentTest(unittest.TestCase):
    def test_flags_private_path_key_and_game_file(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'paths.txt').write_text('/' + 'Users/' + 'alice/game')
            (root / 'secret.txt').write_text('-----BEGIN ' + 'PRIVATE KEY-----')
            (root / 'game.xci').write_text('placeholder')
            (root / 'safe.txt').write_text('public source')
            self.assertTrue(checker.inspect_file(root, 'paths.txt'))
            self.assertTrue(checker.inspect_file(root, 'secret.txt'))
            self.assertTrue(checker.inspect_file(root, 'game.xci'))
            self.assertEqual(checker.inspect_file(root, 'safe.txt'), [])

    def test_flags_symlink(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'link').symlink_to('missing')
            self.assertTrue(checker.inspect_file(root, 'link'))


if __name__ == '__main__':
    unittest.main()
