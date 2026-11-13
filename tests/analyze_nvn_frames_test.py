#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("analyze_nvn_frames", ROOT / "scripts/experiments/analyze_nvn_frames.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class FrameSummaryTests(unittest.TestCase):
    def test_delayed_metrics_and_unknowns(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "frames.tsv"
            path.write_text("# comment\nframe\tname\tvalue\n1\t@frame_ms\t150\n"
                            "1\t@sub_wall_ms\t100\n1\t@sub_cpu_ms\t25\n1\t@tex_hash_ms\t12\n"
                            "2\t@frame_ms\t101\n1\t@gpu_ms\t-1\n"
                            "2\t@pso_new\t0\n2\t@upload_ms\tbroken\n")
            result = module.summarize(path, 100, 10)
            self.assertEqual(result["frames_over_threshold"], 2)
            first, second = result["longest_frames"]
            self.assertEqual(first["submit_non_cpu_ms"], 75)
            self.assertEqual(first["texture_hash_wall_ms"], 12)
            self.assertIsNone(first["texture_hash_cpu_ms"])
            self.assertIsNone(first["gpu_ms"])
            self.assertIsNone(first["new_pipelines"])
            self.assertEqual(second["new_pipelines"], 0)
            self.assertIsNone(second["upload_wall_ms"])

    def test_rejects_invalid_threshold(self):
        with self.assertRaises(ValueError):
            module.summarize(Path("unused"), float("nan"), 10)


if __name__ == "__main__":
    unittest.main()
