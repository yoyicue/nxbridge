#!/usr/bin/env python3
"""Install the session-bound controller adapter in a disposable pinned Sunshine checkout."""
import argparse
from pathlib import Path
import shutil
import subprocess

HERE = Path(__file__).resolve().parent


def replace(text, before, after):
    if text.count(before) != 1: raise RuntimeError('Sunshine input.cpp anchor missing or ambiguous')
    return text.replace(before, after, 1)


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--sunshine', type=Path, required=True)
    a = p.parse_args()
    if subprocess.check_output(['git','-C',str(a.sunshine),'rev-parse','HEAD'],text=True).strip() != '63d35f702ee9e362e43263742981836ec0710384':
        raise RuntimeError('Sunshine revision mismatch')
    if subprocess.check_output(['git','-C',str(a.sunshine),'status','--porcelain','--','src/input.cpp'],text=True).strip():
        raise RuntimeError('Sunshine input.cpp is modified; use a disposable clean source tree')
    src = a.sunshine / 'src/input.cpp'
    target = a.sunshine / 'src/nxbridge_input.hpp'
    if target.exists(): raise RuntimeError('refusing to overwrite existing adapter')
    s = src.read_text()
    s = replace(s, '#include "input.h"', '#include "input.h"\n#include "nxbridge_input.hpp"')
    s = replace(s, '    // Keep track of alt+ctrl+shift key combo', '    bool nxbridge_enabled = false;\n    std::unique_ptr<nxbridge::sender> nxbridge_sender;\n\n    // Keep track of alt+ctrl+shift key combo')
    s = replace(s, '''  void destroy_gamepads(const std::shared_ptr<input_t> &input) {
    for (auto &gamepad''', '''  void destroy_gamepads(const std::shared_ptr<input_t> &input) {
    if (input->nxbridge_sender) input->nxbridge_sender->stop();
    for (auto &gamepad''')
    s = replace(s, '''  void rebind_input(const std::shared_ptr<input_t> &input, const safe::mail_t &mail) {
    input->touch_port_event''', '''  void rebind_input(const std::shared_ptr<input_t> &input, const safe::mail_t &mail) {
    if (input->nxbridge_sender) input->nxbridge_sender->start();
    input->touch_port_event''')
    s = replace(s, '''  void passthrough(std::shared_ptr<input_t> &input, PSS_CONTROLLER_ARRIVAL_PACKET packet) {
    if (!config::input.controller) {''', '''  void passthrough(std::shared_ptr<input_t> &input, PSS_CONTROLLER_ARRIVAL_PACKET packet) {
    if (input->nxbridge_enabled) return;
    if (!config::input.controller) {''')
    anchor = '''  void passthrough(std::shared_ptr<input_t> &input, PNV_MULTI_CONTROLLER_PACKET packet) {
    if (!config::input.controller) {
      return;
    }
'''
    added = anchor + '''
    if (input->nxbridge_enabled) {
      if (input->nxbridge_sender && packet->controllerNumber == 0) {
        if (packet->activeGamepadMask & 1) {
          input->nxbridge_sender->update(nxbridge::convert(static_cast<std::uint16_t>(packet->buttonFlags),
              packet->leftTrigger, packet->rightTrigger, packet->leftStickX, packet->leftStickY,
              packet->rightStickX, packet->rightStickY));
        } else input->nxbridge_sender->update({});
      }
      return;
    }
'''
    s = replace(s, anchor, added)
    s = replace(s, '''    // Do not allow batching if the button state changes on this controller
    if (dest->buttonFlags != src->buttonFlags || dest->buttonFlags2 != src->buttonFlags2)''', '''    // A trigger threshold transition is a ZL/ZR button edge for NXBridge.
    if (nxbridge::configured() && ((dest->leftTrigger >= 128) != (src->leftTrigger >= 128) ||
                                   (dest->rightTrigger >= 128) != (src->rightTrigger >= 128))) {
      return batch_result_e::terminate_batch;
    }
    // Do not allow batching if the button state changes on this controller
    if (dest->buttonFlags != src->buttonFlags || dest->buttonFlags2 != src->buttonFlags2)''')
    s = replace(s, '''  void reset_input_state(const std::shared_ptr<input_t> &input) {
    reset_mouse_buttons();''', '''  void reset_input_state(const std::shared_ptr<input_t> &input) {
    if (input->nxbridge_sender) input->nxbridge_sender->stop();
    reset_mouse_buttons();''')
    s = replace(s, '''        state.inputs.try_emplace(std::move(session_id), input);''', '''        input->nxbridge_enabled = nxbridge::configured();
        if (input->nxbridge_enabled && nxbridge::allowed(session_id)) {
          BOOST_LOG(info) << "NXBridge: paired client selected for game input";
          input->nxbridge_sender = std::make_unique<nxbridge::sender>();
          input->nxbridge_sender->start();
        } else if (input->nxbridge_enabled) {
          BOOST_LOG(warning) << "NXBridge: stream client does not match the configured game session";
        }
        state.inputs.try_emplace(std::move(session_id), input);''')
    target.write_text((HERE/'sunshine_bridge.hpp').read_text())
    src.write_text(s)

if __name__ == '__main__': main()
