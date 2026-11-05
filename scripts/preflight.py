#!/usr/bin/env python3
"""Read-only local inventory; optional ADB reads require an explicit serial."""
from __future__ import annotations

import argparse
import datetime as dt
import json
import platform
import re
import shutil
import subprocess


def run(*args: str, timeout: int = 20) -> dict:
    try:
        result = subprocess.run(args, capture_output=True, text=True, timeout=timeout, check=False)
        return {"returncode": result.returncode, "stdout": result.stdout.strip(), "stderr": result.stderr.strip()}
    except (OSError, subprocess.TimeoutExpired) as exc:
        return {"returncode": None, "stdout": "", "stderr": str(exc)}


def version_tuple(value: str) -> tuple[int, ...]:
    match = re.match(r"(\d+(?:\.\d+)*)", value)
    return tuple(int(item) for item in match.group(1).split(".")) if match else ()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--adb-serial", help="Read only this explicitly selected Android device")
    args = parser.parse_args()
    report = {
        "schema_version": 1,
        "observed_at": dt.datetime.now().astimezone().isoformat(),
        "read_only": True,
        "host": {"system": platform.system(), "architecture": platform.machine()},
        "tools": {name: shutil.which(name) for name in ("git", "python3", "adb", "xcodebuild", "xcrun")},
        "reference_defaults": {"macos_minimum": "26.6", "xcode_version": "27.2"},
        "runtime_benchmark": "not_run",
        "measured_session_capacity": None,
        "notes": ["Inventory does not establish game compatibility, latency or streaming capacity."],
    }
    if platform.system() == "Darwin":
        report["host"]["os_version"] = run("sw_vers", "-productVersion")["stdout"]
        hardware = run("system_profiler", "SPHardwareDataType", "-json")
        if hardware["returncode"] == 0:
            try:
                data = json.loads(hardware["stdout"])["SPHardwareDataType"][0]
                # Deliberately omit serial numbers and platform UUIDs.
                report["host"]["hardware"] = {key: data.get(key) for key in
                    ("machine_name", "machine_model", "chip_type", "physical_memory", "number_processors")}
            except (ValueError, KeyError, IndexError, TypeError) as exc:
                report["host"]["hardware_error"] = str(exc)
        else:
            report["host"]["hardware_error"] = hardware["stderr"]
        xcode = run("xcodebuild", "-version")
        report["host"]["xcode"] = xcode
        match = re.search(r"^Xcode (\S+)", xcode["stdout"], re.MULTILINE)
        xcode_version = match.group(1) if match else ""
        os_version = report["host"]["os_version"]
        report["host"]["matches_reference_defaults"] = bool(
            version_tuple(os_version) >= (26, 6) and xcode_version == "27.2")
        report["host"]["metal_tool"] = run("xcrun", "--find", "metal")
        report["notes"].append("Default upstream toolchain is a reference, not an instruction to upgrade this Mac.")
    if args.adb_serial:
        adb = report["tools"]["adb"]
        if not adb:
            report["android"] = {"status": "unavailable", "reason": "adb not found"}
        else:
            prefix = [adb, "-s", args.adb_serial]
            state = run(*prefix, "get-state", timeout=10)
            device = {"serial": args.adb_serial, "state": state}
            if state["returncode"] == 0 and state["stdout"] == "device":
                device["properties"] = {}
                for prop in ("ro.product.model", "ro.soc.model", "ro.board.platform", "ro.build.version.release"):
                    device["properties"][prop] = run(*prefix, "shell", "getprop", prop, timeout=10)["stdout"]
                memory = run(*prefix, "shell", "cat", "/proc/meminfo", timeout=10)
                device["memory"] = {key: int(value) for key, value in
                    re.findall(r"^(MemTotal|MemAvailable|SwapTotal|SwapFree):\s+(\d+) kB", memory["stdout"], re.MULTILINE)}
                codecs = run(*prefix, "shell", "cat", "/vendor/etc/media_codecs_c2.xml", timeout=10)
                device["declared_hardware_decoders"] = sorted(set(re.findall(
                    r'name="(c2\.mtk\.(?:avc|hevc)\.decoder)"', codecs["stdout"])))
                device["decoder_performance"] = "not_measured"
            report["android"] = device
    else:
        report["android"] = {"status": "not_queried", "reason": "No explicit --adb-serial"}
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
