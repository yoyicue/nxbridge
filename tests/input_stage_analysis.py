#!/usr/bin/env python3
"""Analyze isolated ZL stage probes; timestamps are from one host monotonic clock.

The caller must supply independently observed, ordered game-input edge samples.
This is not an input-to-photon or cross-device latency measurement.
"""
import argparse
import json
import math
import re
import statistics
from pathlib import Path

PATTERN = re.compile(r"NXB_STAGE (arrive|send_start|sent) (\d+) (\d+)(?: (\d+))?")
FIELDS = ("receive_to_forward_ms", "forward_to_game_ms", "receive_to_game_ms")


def parse_events(text):
    events = []
    for line in text.splitlines():
        match = PATTERN.search(line)
        if not match:
            continue
        kind, ns, value, buttons = match.groups()
        if (kind == "arrive") != (buttons is None):
            raise ValueError("malformed diagnostic event")
        events.append((kind, int(ns), int(value), int(buttons) if buttons is not None else None))
    return events


def describe(values):
    if not values:
        return None
    values = sorted(values)
    return {"count": len(values), "min": values[0], "p50": statistics.median(values),
            "p95_nearest_rank": values[math.ceil(.95 * len(values)) - 1],
            "p99_nearest_rank": values[math.ceil(.99 * len(values)) - 1], "max": values[-1]}


def analyze(events, samples):
    results = []
    previous_game = 0
    for sample in samples:
        for phase, expected, game in (("press", 1, sample["pressed_ns"]),
                                      ("release", 0, sample["released_ns"])):
            item = {"iteration": sample["iteration"], "phase": phase, "game_ns": game}
            if not isinstance(game, int) or isinstance(game, bool) or game <= previous_game:
                raise ValueError("game sample timestamps must be strictly increasing")
            arrivals = [e for e in events if e[0] == "arrive" and previous_game < e[1] <= game]
            candidates = [e for e in arrivals if e[2] == expected]
            if not candidates:
                item["error"] = "missing matching arrival"
            else:
                arrive = candidates[0][1]
                if any(e[2] != expected and e[1] >= arrive for e in arrivals):
                    item["error"] = "ambiguous input transitions before sampling"
                else:
                    sends = [e for e in events if e[0] == "send_start" and arrive <= e[1] <= game
                             and bool(e[3] & 256) == bool(expected)]
                    if not sends:
                        item["error"] = "missing matching send-start"
                    else:
                        sent = sends[0]
                        done = next((e for e in events if e[0] == "sent" and e[2] == sent[2]), None)
                        item.update(arrival_ns=arrive, forward_start_ns=sent[1],
                                    forward_done_ns=done[1] if done else None,
                                    receive_to_forward_ms=(sent[1] - arrive) / 1e6,
                                    forward_to_game_ms=(game - sent[1]) / 1e6,
                                    receive_to_game_ms=(game - arrive) / 1e6,
                                    matching_arrivals_before_sample=len(candidates))
            results.append(item)
            previous_game = game
    summary = {}
    for phase in ("press", "release"):
        valid = [x for x in results if x["phase"] == phase and "error" not in x]
        summary[phase] = {field: describe([x[field] for x in valid]) for field in FIELDS}
    return {"schema": 1, "clock": "host CLOCK_MONOTONIC",
            "pairing": "first matching arrival after prior game sample, then first matching send-start before game sample",
            "errors": [x for x in results if "error" in x], "summary": summary, "samples": results}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server-log", required=True, type=Path)
    parser.add_argument("--samples", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    events = parse_events(args.server_log.read_text(errors="replace"))
    samples = json.loads(args.samples.read_text())["samples"]
    result = analyze(events, samples)
    with args.output.open("x") as output:
        json.dump(result, output, indent=2)
        output.write("\n")
    print(f"matched {len(result['samples']) - len(result['errors'])}/{len(result['samples'])}; errors={len(result['errors'])}")
    return 1 if result["errors"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
