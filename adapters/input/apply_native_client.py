#!/usr/bin/env python3
"""Switch the experimental overlay back to unmodified Moonlight controller transport."""
import argparse
from pathlib import Path
import subprocess

p = argparse.ArgumentParser()
p.add_argument('--moonlight', type=Path, required=True)
a = p.parse_args()
sha = subprocess.check_output(['git','-C',str(a.moonlight),'rev-parse','HEAD'],text=True).strip()
if sha != 'b48494cb96bff23d8886c4775cc4f39a1075495d': raise RuntimeError('Moonlight revision mismatch')
package = a.moonlight / 'app/src/main/java/com/limelight/binding/input/virtual_controller'
sender = package / 'NxbridgeSender.java'
controller = package / 'VirtualController.java'
build = a.moonlight / 'app/build.gradle'
sender_source = sender.read_text()
controller_source = controller.read_text()
version = '317' if 'interface Ready' in sender_source else '316'
if ('NxbridgeSender' not in controller_source or
        (version == '316' and 'new InetSocketAddress("127.0.0.1", PORT)' not in sender_source) or
        build.read_text().count('versionCode = ' + version) != 1):
    raise RuntimeError('expected the isolated NXB-045 input experiment')
original = subprocess.check_output(['git','-C',str(a.moonlight),'show','HEAD:app/src/main/java/com/limelight/binding/input/virtual_controller/VirtualController.java'],text=True)
controller.write_text(original)
sender.unlink()
build.write_text(build.read_text().replace('versionCode = ' + version,'versionCode = 318'))
