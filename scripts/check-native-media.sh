#!/bin/sh
# Device-free macOS regression tests for the experimental media adapter.
set -eu

if [ "$(uname -s)" != Darwin ]; then
    echo "Native media tests require macOS." >&2
    exit 1
fi
work_dir=$(mktemp -d "${TMPDIR:-/tmp}/nxbridge-native-tests.XXXXXXXX")
trap 'rm -rf -- "$work_dir"' EXIT HUP INT TERM

clang -std=c11 -Wall -Wextra -Werror -fsanitize=address,undefined -pthread \
    tests/nxnative_pcm_queue.c adapters/nxnative/pcm_queue.c -o "$work_dir/pcm"
"$work_dir/pcm"
clang -std=c11 -Wall -Wextra -Werror -Icrates/nxbridge-media/include \
    tests/nxnative_mach_wire.c -o "$work_dir/wire"
"$work_dir/wire"
clang -std=gnu11 -Wall -Wextra -Werror tests/nxnative_iosurface_mach.m \
    -o "$work_dir/iosurface" -framework Foundation -framework IOSurface
"$work_dir/iosurface"
for name in nxnative_mach_duplicate nxnative_audio_backpressure nxnative_video_queue; do
    clang -std=gnu11 -Wall -Wextra -Werror -fblocks -Icrates/nxbridge-media/include \
        "tests/$name.m" -o "$work_dir/$name" \
        -framework Foundation -framework CoreMedia -framework CoreVideo \
        -framework IOSurface -framework VideoToolbox
    "$work_dir/$name"
done
echo "PASS: device-free native media regressions"
