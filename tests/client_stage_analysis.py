#!/usr/bin/env python3
"""Analyze an isolated Android ZL callback probe; never subtract host and client clocks."""
import argparse
import json
import math
import re
import statistics
from pathlib import Path

PATTERN = re.compile(r"NXBClientStage\(\s*\d+\): v1 action=(\d+) event_ms=(\d+) entry_ms=(\d+) dispatch_ms=(-?\d+) to_call_ns=(-?\d+) callback_ns=(\d+)")


def parse(text):
    result = []
    for line in text.splitlines():
        match = PATTERN.search(line)
        if match:
            action, event, entry, delay, call, callback = map(int, match.groups())
            result.append({"action": action, "event_ms": event, "entry_ms": entry,
                           "dispatch_ms": delay, "to_call_ns": call, "callback_ns": callback})
    return result


def stats(values):
    values = sorted(values)
    if not values:
        return None
    return {"count": len(values), "min": values[0], "p50": statistics.median(values),
            "p95_nearest_rank": values[math.ceil(.95 * len(values)) - 1],
            "p99_nearest_rank": values[math.ceil(.99 * len(values)) - 1], "max": values[-1]}


def analyze(rows, expected_pairs):
    errors = []
    if expected_pairs <= 0 or len(rows) != expected_pairs * 2:
        errors.append("unexpected edge count")
    previous_event = -1
    for index, row in enumerate(rows):
        if row["action"] != index % 2:
            errors.append(f"edge {index}: unexpected action order")
        if row["event_ms"] < previous_event or row["dispatch_ms"] < 0 or row["entry_ms"] - row["event_ms"] != row["dispatch_ms"]:
            errors.append(f"edge {index}: invalid uptime timestamps")
        if row["to_call_ns"] < 0 or row["to_call_ns"] > row["callback_ns"]:
            errors.append(f"edge {index}: missing or invalid send-call boundary")
        previous_event = row["event_ms"]
    summary = {}
    for action, name in ((0, "down"), (1, "up")):
        subset = [x for x in rows if x["action"] == action and x["dispatch_ms"] >= 0 and
                  0 <= x["to_call_ns"] <= x["callback_ns"]]
        summary[name] = {"dispatch_ms": stats([x["dispatch_ms"] for x in subset]),
                         "entry_to_send_call_ms": stats([x["to_call_ns"] / 1e6 for x in subset]),
                         "callback_total_ms": stats([x["callback_ns"] / 1e6 for x in subset])}
    return {"schema": 1, "errors": errors, "summary": summary, "samples": rows}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--client-log", required=True, type=Path)
    parser.add_argument("--expected-pairs", required=True, type=int)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    result = analyze(parse(args.client_log.read_text(errors="replace")), args.expected_pairs)
    with args.output.open("x") as output:
        json.dump(result, output, indent=2)
        output.write("\n")
    print(f"{len(result['samples'])} edges; errors={len(result['errors'])}")
    return 1 if result["errors"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
