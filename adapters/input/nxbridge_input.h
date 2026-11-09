#pragma once
#include <stdint.h>
/* With a configured session, replace player 0's unassigned host input with
 * the next bounded remote snapshot (including neutral on receiver failure).
 * When unconfigured, leave the caller's local input unchanged. */
void nxbridge_input_sample(uint64_t *buttons, int32_t axes[4]);
void nxbridge_input_trace(uint64_t buttons, const int32_t axes[4]);
