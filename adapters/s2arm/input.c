/* SPDX-License-Identifier: MIT
 * NXBridge's process-local input source for the headless S2Arm host.
 * Build without S2A_MAC_FRONTEND: only this session supplies controller input.
 */
#include <stdint.h>
#include "nxbridge_input.h"

__attribute__((visibility("default"), used))
uint32_t s2a_vpad_read(int32_t sticks[4]) {
    uint64_t buttons = 0;
    for (int i = 0; i < 4; ++i) sticks[i] = 0;
    nxbridge_input_sample(&buttons, sticks);
    nxbridge_input_trace(buttons, sticks);
    return (uint32_t)buttons;
}
