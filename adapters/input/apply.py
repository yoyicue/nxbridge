#!/usr/bin/env python3
"""Apply the experimental adapters to pinned, disposable source trees."""
import argparse
import pathlib
import re
import shutil
import subprocess

HERE = pathlib.Path(__file__).resolve().parent

def replace_one(path, before, after):
    text = path.read_text()
    if text.count(before) != 1:
        raise RuntimeError(f"expected exactly one patch anchor in {path}")
    path.write_text(text.replace(before, after, 1))

def main():
    p = argparse.ArgumentParser()
    p.add_argument('--engine', type=pathlib.Path, required=True)
    p.add_argument('--moonlight', type=pathlib.Path, required=True)
    p.add_argument('--token-file', type=pathlib.Path, required=True)
    p.add_argument('--port', type=int, required=True)
    a = p.parse_args()
    expected = {'engine': 'b8d16169c5fb7a3c6a936a950e294b1df4680551', 'moonlight': 'b48494cb96bff23d8886c4775cc4f39a1075495d'}
    for name in expected:
        path = getattr(a, name)
        sha = subprocess.check_output(['git', '-C', str(path), 'rev-parse', 'HEAD'], text=True).strip()
        if sha != expected[name]: raise RuntimeError(f'{name} revision mismatch: {sha}')
    targets = {
        'engine': ['dm/host/hle_hid.c'],
        'moonlight': ['build.gradle', 'app/build.gradle',
            'app/src/main/java/com/limelight/binding/input/virtual_controller/VirtualController.java',
            'app/src/main/java/com/limelight/binding/input/virtual_controller/VirtualControllerConfigurationLoader.java',
            'app/src/main/java/com/limelight/preferences/PreferenceConfiguration.java'],
    }
    for name, paths in targets.items():
        dirty = subprocess.check_output(['git', '-C', str(getattr(a, name)), 'status', '--porcelain', '--', *paths], text=True)
        if dirty: raise RuntimeError(f'{name}: a target file is already modified; use a disposable clean source tree')
    for path in [a.engine / 'dm/host/hle_nxbridge_input.c', a.engine / 'dm/host/nxbridge_input.h',
                 a.moonlight / 'app/src/main/java/com/limelight/binding/input/virtual_controller/NxbridgeSender.java']:
        if path.exists(): raise RuntimeError(f'refusing to overwrite {path}')
    token = a.token_file.read_text().strip()
    if not re.fullmatch('[a-f0-9]{64}', token) or not 1024 <= a.port <= 65535:
        raise RuntimeError('invalid token or port')
    host = a.engine / 'dm/host'
    hid = host / 'hle_hid.c'
    replace_one(hid, '#include "dm_input.h"', '#include "dm_input.h"\n#include "nxbridge_input.h"')
    replace_one(hid, '    if (!pl) {   // the physical sources so far', '    if (!pl) nxbridge_input_sample(&b, a);\n    if (!pl) {   // the physical sources so far')
    replace_one(hid, '    s->buttons = b;\n    s->attributes = ATTR_CONNECTED', '    s->buttons = b;\n    if (!pl) nxbridge_input_trace(s->buttons, a);\n    s->attributes = ATTR_CONNECTED')
    shutil.copyfile(HERE / 'nxbridge_input.h', host / 'nxbridge_input.h')
    shutil.copyfile(HERE / 'nxbridge_input.c', host / 'hle_nxbridge_input.c')
    package = a.moonlight / 'app/src/main/java/com/limelight/binding/input/virtual_controller'
    v = package / 'VirtualController.java'
    replace_one(v, '    private FrameLayout frame_layout = null;', '    private NxbridgeSender nxbridgeSender;\n    private FrameLayout frame_layout = null;')
    replace_one(v, '    public void hide() {', '    public void hide() {\n        handler.removeCallbacks(delayedRetransmitRunnable);\n        inputContext = new ControllerInputContext();\n        if (nxbridgeSender != null) { nxbridgeSender.close(); nxbridgeSender = null; }')
    replace_one(v, '    public void show() {', '    public void show() {\n        if (nxbridgeSender == null) nxbridgeSender = new NxbridgeSender();')
    replace_one(v, '    private void sendControllerInputContextInternal() {', '    private void sendControllerInputContextInternal() {\n        if (nxbridgeSender != null) nxbridgeSender.send(inputContext);')
    (package / 'NxbridgeSender.java').write_text((HERE / 'NxbridgeSender.java').read_text().replace('@TOKEN@', token).replace('@PORT@', str(a.port)))
    layout = package / 'VirtualControllerConfigurationLoader.java'
    text = layout.read_text()
    text = text.replace('config.flipFaceButtons', 'true')
    for old, new in [('"LT"','"ZL"'),('"RT"','"ZR"'),('"LB"','"L"'),('"RB"','"R"'),('"BACK"','"−"'),('"START"','"+"')]:
        text = text.replace(old, new)
    layout.write_text(text)
    prefs = a.moonlight / 'app/src/main/java/com/limelight/preferences/PreferenceConfiguration.java'
    text = prefs.read_text()
    text = re.sub(r'config\.showGuideButton = prefs\.getBoolean\(SHOW_GUIDE_BUTTON_PREF_STRING, SHOW_GUIDE_BUTTON_DEFAULT\);', 'config.showGuideButton = false;', text)
    text = re.sub(r'config\.onscreenController = prefs\.getBoolean\(ONSCREEN_CONTROLLER_PREF_STRING, ONSCREEN_CONTROLLER_DEFAULT\);', 'config.onscreenController = true;', text)
    prefs.write_text(text)
    build = a.moonlight / 'build.gradle'
    replace_one(build, 'com.android.tools.build:gradle:9.4.0', 'com.android.tools.build:gradle:9.1.1')
    appbuild = a.moonlight / 'app/build.gradle'
    replace_one(appbuild, 'ndkVersion "29.0.14206865"', 'ndkVersion "28.2.13676358"')
    replace_one(appbuild, '    defaultConfig {', '    defaultConfig {\n        ndk { abiFilters "arm64-v8a" }')
    replace_one(appbuild, 'versionCode = 315', 'versionCode = 316')
    replace_one(appbuild, 'resValue "string", "app_label", "Moonlight (Debug)"', 'resValue "string", "app_label", "NXBridge Pad"')
if __name__ == '__main__': main()
