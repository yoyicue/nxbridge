#!/usr/bin/env python3
"""Install the NXB-035 experiments into a disposable pinned nxnative checkout."""
from pathlib import Path
import subprocess
import sys

PATCHES = {
    "b8d16169c5fb7a3c6a936a950e294b1df4680551": ("pcm-probe.patch", "video-probe.patch", "input-integration.patch"),
    "8d44aaf8f3a59eb0e0f63ced14b3f7f26e903484": ("integration-8d44aaf.patch",),
}
HERE = Path(__file__).resolve().parent
if len(sys.argv) != 2:
    raise SystemExit("usage: apply-experiment.py PATH_TO_DISPOSABLE_NXNATIVE_CHECKOUT")
root = Path(sys.argv[1]).resolve()
head = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
if head not in PATCHES:
    raise SystemExit("Unexpected nxnative revision; refusing to modify checkout")
files = {
    "pcm_queue.h": "dm/host/pcm_queue.h",
    "pcm_queue.c": "dm/host/hle_nxbridge_pcm_queue.c",
    "pcm_probe.c": "dm/host/hle_nxbridge_pcm_probe.c",
    "video_probe.h": "dm/gfx/nvn/nxbridge_video_probe.h",
    "video_probe.m": "dm/gfx/nvn/nxbridge_video_probe.m",
    "mach_media.h": "dm/gfx/nvn/mach_media.h",
}
for target in files.values():
    if (root / target).exists():
        raise SystemExit(f"Refusing to overwrite existing {target}")
for target in ("dm/host/nxbridge_input.h", "dm/host/hle_nxbridge_input.c"):
    if (root / target).exists():
        raise SystemExit(f"Refusing to overwrite existing {target}")
if (root / "dm/host/mach_media.h").exists():
    raise SystemExit("Refusing to overwrite existing dm/host/mach_media.h")
for patch in PATCHES[head]:
    subprocess.run(["git", "-C", str(root), "apply", "--check", str(HERE / patch)], check=True)
for source, target in files.items():
    text = (HERE / source).read_text()
    if source == "video_probe.m":
        text = text.replace('#import "video_probe.h"', '#import "nxbridge_video_probe.h"')
    (root / target).write_text(text)
(root / "dm/host/nxbridge_input.h").write_bytes((HERE.parent / "input/nxbridge_input.h").read_bytes())
(root / "dm/host/hle_nxbridge_input.c").write_bytes((HERE.parent / "input/nxbridge_input.c").read_bytes())
(root / "dm/host/mach_media.h").write_bytes((HERE / "mach_media.h").read_bytes())
for patch in PATCHES[head]:
    subprocess.run(["git", "-C", str(root), "apply", str(HERE / patch)], check=True)
print("Applied NXB-035 experiments to the disposable checkout")
