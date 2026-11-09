#!/usr/bin/env python3
import unittest
from client_stage_analysis import analyze, parse


class ClientStageAnalysisTest(unittest.TestCase):
    def test_pairs_and_units(self):
        lines = ("I/NXBClientStage(123): v1 action=0 event_ms=100 entry_ms=102 dispatch_ms=2 to_call_ns=50000 callback_ns=80000\n"
                 "I/NXBClientStage(123): v1 action=1 event_ms=110 entry_ms=110 dispatch_ms=0 to_call_ns=30000 callback_ns=60000\n")
        result = analyze(parse(lines), 1)
        self.assertEqual(result['errors'], [])
        self.assertEqual(result['summary']['down']['entry_to_send_call_ms']['p50'], .05)
        self.assertEqual(result['summary']['up']['dispatch_ms']['p50'], 0)

    def test_missing_call_and_bad_order_are_not_success(self):
        row = {'action': 1, 'event_ms': 2, 'entry_ms': 1, 'dispatch_ms': -1,
               'to_call_ns': -1, 'callback_ns': 100}
        result = analyze([row], 1)
        self.assertGreaterEqual(len(result['errors']), 3)
        self.assertIsNone(result['summary']['up']['entry_to_send_call_ms'])

    def test_wrong_declared_dispatch_is_rejected(self):
        row = {'action': 0, 'event_ms': 2, 'entry_ms': 4, 'dispatch_ms': 1,
               'to_call_ns': 10, 'callback_ns': 20}
        self.assertTrue(analyze([row], 1)['errors'])


if __name__ == '__main__':
    unittest.main()
