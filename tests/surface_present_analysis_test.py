#!/usr/bin/env python3
import unittest
from surface_present_analysis import analyze


def snap(*times):
    return {'rows': [[t, t, t] for t in times]}


class SurfacePresentAnalysisTest(unittest.TestCase):
    def test_deduplicates_overlapping_ring_buffer(self):
        out = analyze({'mode': 'test', 'snapshots': [snap(100, 200), snap(100, 200, 300), snap(200, 300, 400)]})
        self.assertEqual(out['frame_count'], 2)
        self.assertEqual(out['interval_count'], 1)
        self.assertEqual(out['snapshot_gaps'], [])
        self.assertEqual(out['interval_ms']['maximum'], .0001)

    def test_missing_overlap_is_reported(self):
        out = analyze({'mode': 'test', 'snapshots': [snap(100, 200), snap(300, 400)]})
        self.assertEqual(out['snapshot_gaps'], [1])

    def test_empty_baseline_does_not_become_zero_fps(self):
        with self.assertRaises(ValueError):
            analyze({'mode': 'test', 'snapshots': [snap(0, (1 << 63)-1), snap(300)]})


if __name__ == '__main__':
    unittest.main()
