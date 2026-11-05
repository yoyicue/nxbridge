#!/usr/bin/env python3
"""Exercise the synthetic source and a local FFmpeg round trip without a game."""
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def run(command):
    print('+', ' '.join(map(str, command)), flush=True)
    subprocess.run(command, cwd=ROOT, check=True)


def main():
    with tempfile.TemporaryDirectory(prefix='nxbridge-synthetic-') as temp:
        folder = Path(temp)
        cargo = ['cargo', 'run', '--locked']
        if os.environ.get('NXB_CARGO_OFFLINE') == '1':
            cargo.append('--offline')
        run(cargo + ['-p', 'nxbridge-stream', '--bin', 'synthetic-fixture', '--', str(folder)])
        video = folder / 'video.bgra'
        audio = folder / 'audio.f32le'
        if video.stat().st_size != 60 * 320 * 180 * 4 or audio.stat().st_size != 2 * 48000 * 2 * 4:
            raise ValueError('synthetic audio or video length is incorrect')
        with video.open('rb') as f:
            first = f.read(320 * 180 * 4)
            f.seek(30 * 320 * 180 * 4)
            later = f.read(320 * 180 * 4)
        if first == later:
            raise ValueError('synthetic video frames did not change')
        pcm = audio.read_bytes()
        pulse = struct.unpack_from('<f', pcm, 8 * 100)[0]
        silence = struct.unpack_from('<f', pcm, 8 * 24000)[0]
        pulse2 = struct.unpack_from('<f', pcm, 8 * 48100)[0]
        if abs(pulse) < 0.01 or abs(silence) > 1e-7 or abs(pulse2) < 0.01:
            raise ValueError('synthetic audio pulse or silence is missing')
        output = folder / 'roundtrip.mkv'
        run(['ffmpeg', '-nostdin', '-v', 'error', '-f', 'rawvideo', '-pixel_format', 'bgra',
             '-video_size', '320x180', '-framerate', '30', '-i', str(video),
             '-f', 'f32le', '-ar', '48000', '-ac', '2', '-i', str(audio),
             '-c:v', 'ffv1', '-c:a', 'pcm_s16le', str(output)])
        probe = subprocess.run(['ffprobe', '-v', 'error', '-count_frames', '-show_streams',
                                '-of', 'json', str(output)], check=True, capture_output=True, text=True)
        streams = json.loads(probe.stdout)['streams']
        v = next(s for s in streams if s['codec_type'] == 'video')
        a = next(s for s in streams if s['codec_type'] == 'audio')
        if (v.get('codec_name'), v.get('width'), v.get('height'), v.get('nb_read_frames')) != ('ffv1', 320, 180, '60'):
            raise ValueError('encoded video properties are incorrect')
        if (a.get('codec_name'), a.get('sample_rate'), a.get('channels')) != ('pcm_s16le', '48000', 2):
            raise ValueError('encoded audio properties are incorrect')
        run(['ffmpeg', '-nostdin', '-v', 'error', '-i', str(output), '-f', 'null', '-'])
    print('PASS: 60 changing frames, two audio pulses, FFV1/PCM encode and decode.')
    print('Local fixture only: no GameStream, Moonlight, production codec, game or performance validation.')


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as exc:
        print(f'Self-check failed: {exc}', file=sys.stderr)
        sys.exit(1)
