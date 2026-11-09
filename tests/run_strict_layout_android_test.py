#!/usr/bin/env python3
"""Compile the real layout parser and exercise it on one explicitly selected Android device."""
import argparse
import os
import subprocess
import tempfile
import uuid
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True, help='explicit adb device serial')
    parser.add_argument('--client', required=True, type=Path, help='patched Moonlight Android source')
    parser.add_argument('--android-jar', required=True, type=Path)
    parser.add_argument('--d8', required=True, type=Path)
    parser.add_argument('--java-home', type=Path, help='optional JDK directory for javac and d8')
    args = parser.parse_args()
    if not args.serial or any(c.isspace() for c in args.serial):
        parser.error('invalid serial')
    source = args.client / 'app/src/main/java/com/limelight/binding/input/virtual_controller'
    environment = os.environ.copy()
    javac = 'javac'
    if args.java_home:
        environment['JAVA_HOME'] = str(args.java_home)
        javac = str(args.java_home / 'bin/javac')
    adb = ['adb', '-s', args.serial]
    device_file = '/data/local/tmp/nxb-strict-json-' + uuid.uuid4().hex + '.jar'
    with tempfile.TemporaryDirectory(prefix='nxb-strict-json-') as tmp:
        directory = Path(tmp); classes = directory / 'classes'; dex = directory / 'dex'
        classes.mkdir(); dex.mkdir()
        sources = [source / 'PortableLayoutFile.java', source / 'StrictLayoutJson.java', ROOT / 'tests/StrictLayoutJsonAndroidTest.java']
        subprocess.run([javac, '-source', '17', '-target', '17', '-cp', str(args.android_jar), '-d', str(classes), *map(str, sources)], check=True, env=environment)
        class_files = sorted(classes.rglob('*.class'))
        subprocess.run([str(args.d8), '--min-api', '23', '--output', str(dex), *map(str, class_files)], check=True, env=environment)
        jar = directory / 'test.jar'
        with zipfile.ZipFile(jar, 'w') as archive:
            archive.write(dex / 'classes.dex', 'classes.dex')
        if subprocess.run(adb + ['shell', 'test', '!', '-e', device_file]).returncode != 0:
            raise RuntimeError('temporary device path already exists')
        try:
            subprocess.run(adb + ['push', str(jar), device_file], check=True)
            subprocess.run(adb + ['shell', f'CLASSPATH={device_file} app_process /system/bin StrictLayoutJsonAndroidTest'], check=True)
        finally:
            subprocess.run(adb + ['shell', 'rm', '-f', device_file], check=False)


if __name__ == '__main__':
    main()
