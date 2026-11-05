#pragma once
#include <stdint.h>
/* Consume press edges once, preserving current held state. */
void nxbridge_input_sample(uint64_t *buttons, int32_t axes[4]);
void nxbridge_input_trace(uint64_t buttons, const int32_t axes[4]);
