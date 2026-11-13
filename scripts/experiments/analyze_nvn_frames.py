#!/usr/bin/env python3
"""Summarize recorded S2Arm NVN frame timings; this does not establish causality."""
import argparse
import csv
import json
import math
from pathlib import Path


FIELDS = {
    "frame_ms": "@frame_ms",
    "submit_present_ms": "@nvn_ms",
    "submit_wall_ms": "@sub_wall_ms",
    "submit_cpu_ms": "@sub_cpu_ms",
    "process_cpu_ms": "@proc_cpu_ms",
    "upload_wall_ms": "@upload_ms",
    "upload_gpu_ms": "@upload_gpu_ms",
    "texture_create_wall_ms": "@tex_create_ms",
    "texture_residency_wall_ms": "@tex_residency_ms",
    "texture_hash_wall_ms": "@tex_hash_ms",
    "texture_hash_cpu_ms": "@tex_hash_cpu_ms",
    "texture_hash_bytes": "@tex_hash_bytes",
    "gpu_ms": "@gpu_ms",
    "acquire_wait_ms": "@wait_ms",
    "present_queue_wait_ms": "@pq_wait_ms",
    "mprotect_ms": "@mprotect_ms",
    "new_pipelines": "@pso_new",
    "new_shader_libraries": "@lib_new",
    "elapsed_s": "@t_s",
}


def summarize(path: Path, threshold_ms: float, limit: int) -> dict:
    if not math.isfinite(threshold_ms) or threshold_ms < 0 or limit < 1:
        raise ValueError("threshold must be finite and nonnegative; limit must be positive")
    frames = {}
    with path.open(encoding="utf-8") as f:
        for row in csv.reader(f, delimiter="\t"):
            if len(row) != 3 or not row[0].isdigit() or row[1] not in FIELDS.values():
                continue
            try:
                value = float(row[2])
            except ValueError:
                continue
            if not math.isfinite(value):
                continue
            frames.setdefault(int(row[0]), {})[row[1]] = value
    selected = [(n, v) for n, v in frames.items() if v.get("@frame_ms", -1) > threshold_ms]
    selected.sort(key=lambda item: (-item[1]["@frame_ms"], item[0]))
    result = []
    for number, values in selected[:limit]:
        entry = {"frame": number}
        for label, key in FIELDS.items():
            value = values.get(key)
            # Upstream uses -1 for an unavailable GPU / CPU timing.
            entry[label] = value if value is not None and value >= 0 else None
        wall, cpu = entry["submit_wall_ms"], entry["submit_cpu_ms"]
        entry["submit_non_cpu_ms"] = round(wall - cpu, 3) if wall is not None and cpu is not None and wall >= cpu else None
        result.append(entry)
    return {
        "threshold_ms": threshold_ms,
        "frames_with_timing": sum("@frame_ms" in v for v in frames.values()),
        "frames_over_threshold": len(selected),
        "longest_frames": result,
        "note": "Diagnostic measurements overlap; non-CPU time is not proof of a specific wait or cause.",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tsv", type=Path)
    parser.add_argument("--threshold-ms", type=float, default=100.0)
    parser.add_argument("--limit", type=int, default=10)
    args = parser.parse_args()
    print(json.dumps(summarize(args.tsv, args.threshold_ms, args.limit), indent=2))


if __name__ == "__main__":
    main()
