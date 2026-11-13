import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

from PIL import Image

SCRIPT = Path(__file__).resolve().parents[1] / 'scripts/experiments/oot_route_pilot.py'
spec = importlib.util.spec_from_file_location('oot_route_pilot', SCRIPT)
pilot = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pilot)


def png(color):
    data = io.BytesIO()
    Image.new('RGB', (2560, 1600), color).save(data, format='PNG')
    return data.getvalue()


class RoutePilotTests(unittest.TestCase):
    def test_unrecognized_screen_stops_without_input(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            refs = base / 'refs'
            refs.mkdir()
            (refs / 'room1.png').write_bytes(png('white'))
            foreground = 'topResumedActivity=ActivityRecord{a u0 com.limelight.debug/com.limelight.Game} t1}'
            def read(command, **kwargs):
                return foreground if 'dumpsys' in command else png('black')
            argv = ['pilot', '--serial', 'test-serial', '--references', str(refs),
                    '--output', str(base / 'out'), '--stage-timeout', '5']
            ticks = iter(range(100))
            with mock.patch.object(pilot, 'STAGES', [('room1', ('hold', 1, 1, 250))]), \
                 mock.patch.object(sys, 'argv', argv), \
                 mock.patch.object(pilot.subprocess, 'check_output', side_effect=read), \
                 mock.patch.object(pilot.subprocess, 'run') as send, \
                 mock.patch.object(pilot.time, 'monotonic', side_effect=lambda: next(ticks)), \
                 mock.patch.object(pilot.time, 'sleep'), contextlib.redirect_stdout(io.StringIO()):
                with self.assertRaises(TimeoutError):
                    pilot.main()
                send.assert_not_called()
            events = [json.loads(line) for line in (base / 'out/events.jsonl').read_text().splitlines()]
            self.assertEqual(events[-1]['kind'], 'stopped')
            self.assertTrue((base / 'out/timeout-last.png').exists())

    def test_wrong_resolution_fails_closed(self):
        with self.assertRaises(ValueError):
            pilot.normalized(Image.new('RGB', (1280, 720)))

if __name__ == '__main__':
    unittest.main()
