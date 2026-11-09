#!/usr/bin/env python3
import unittest
from input_stage_analysis import analyze, parse_events


class StageAnalysisTest(unittest.TestCase):
    def test_matching_repeated_inputs_and_send_completion(self):
        log = """NXB_STAGE arrive 1000000 1
NXB_STAGE send_start 1100000 7 256
NXB_STAGE sent 1200000 7 256
NXB_STAGE arrive 1500000 1
NXB_STAGE arrive 3000000 0
NXB_STAGE send_start 3100000 8 0
NXB_STAGE sent 3200000 8 0
"""
        got = analyze(parse_events(log), [{"iteration": 1, "pressed_ns": 2000000, "released_ns": 4000000}])
        self.assertEqual(got["errors"], [])
        self.assertEqual(got["samples"][0]["receive_to_game_ms"], 1)
        self.assertEqual(got["samples"][0]["matching_arrivals_before_sample"], 2)
        self.assertEqual(got["samples"][1]["forward_done_ns"], 3200000)

    def test_missing_and_ambiguous_are_not_reported_as_zero_latency(self):
        log = "NXB_STAGE arrive 10 1\nNXB_STAGE arrive 11 0\nNXB_STAGE send_start 12 3 256\n"
        got = analyze(parse_events(log), [{"iteration": 1, "pressed_ns": 20, "released_ns": 30}])
        self.assertEqual(len(got["errors"]), 2)
        self.assertIsNone(got["summary"]["press"]["receive_to_game_ms"])

    def test_reject_out_of_order_and_malformed(self):
        with self.assertRaises(ValueError):
            analyze([], [{"iteration": 1, "pressed_ns": 10, "released_ns": 9}])
        with self.assertRaises(ValueError):
            parse_events("NXB_STAGE send_start 1 2")


if __name__ == "__main__":
    unittest.main()
