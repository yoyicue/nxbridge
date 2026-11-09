#!/usr/bin/env python3
import unittest
from gfxinfo_framestats_analysis import analyze


class FrameStatsTest(unittest.TestCase):
    def test_selects_window_and_excludes_invalid_rows(self):
        text='''Window: other
---PROFILEDATA---
Flags,IntendedVsync,FrameCompleted,
0,100,200,
---PROFILEDATA---
Window: game
---PROFILEDATA---
Flags,IntendedVsync,FrameCompleted,
0,1000000,3000000,
1,1000000,4000000,
0,9000000,8000000,
---PROFILEDATA---
'''
        out=analyze(text,'game')
        self.assertEqual(out['valid_frames'],1)
        self.assertEqual(out['skipped_rows'],2)
        self.assertEqual(out['frame_duration_ms']['p50'],2)

    def test_empty_is_unknown(self):
        out=analyze('Window: game\n---PROFILEDATA---\nFlags,IntendedVsync,FrameCompleted,\n---PROFILEDATA---\n','game')
        self.assertIsNone(out['frame_duration_ms'])

    def test_header_mismatch_is_rejected(self):
        with self.assertRaises(ValueError):analyze('Window: game\n---PROFILEDATA---\nFlags,Other,\n','game')


if __name__ == '__main__':unittest.main()
