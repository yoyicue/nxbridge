/* Convert a validated local Mach frame and its imported IOSurface geometry
 * into the common in-process media ABI. This header is not a wire format. */
#ifndef NXBRIDGE_MACH_ABI_H
#define NXBRIDGE_MACH_ABI_H
#include "mach_media.h"
#include "nxbridge_media.h"

/* color_profile is supplied by the source contract; geometry does not prove
 * the source's transfer function or primaries. */
static inline int nxb_mach_video_descriptors(const nxb_mach_frame *wire,
        const nxb_media_epoch_v1 *expected, uint64_t allocation, uint64_t stride, uint32_t color_profile,
        nxb_video_buffer_v1 *buffer, nxb_video_frame_v1 *frame) {
    if (!wire || !expected || !buffer || !frame ||
        memcmp(wire->session_id, expected->session_id, 16) || !expected->generation ||
        wire->generation != expected->generation || !wire->buffer_id || wire->buffer_id > NXB_VIDEO_BUFFERS_MAX ||
        !wire->sequence || !wire->pts_ns || !wire->duration_ns || !wire->width || !wire->height ||
        wire->width > 8192 || wire->height > 8192 || stride > UINT32_MAX ||
        stride < (uint64_t)wire->width * 4 || stride > allocation / wire->height || color_profile != NXB_COLOR_BT709) return 0;
    unsigned nonzero = 0; for (unsigned i = 0; i < 16; i++) nonzero |= expected->session_id[i];
    if (!nonzero) return 0;
    *buffer = (nxb_video_buffer_v1){0}; *frame = (nxb_video_frame_v1){0};
    buffer->struct_size = sizeof(*buffer); buffer->abi_version = NXB_MEDIA_ABI_V1; buffer->epoch = *expected;
    buffer->buffer_id = wire->buffer_id; buffer->allocation_bytes = allocation; buffer->storage_kind = NXB_STORAGE_IOSURFACE;
    buffer->pixel_format = NXB_VIDEO_BGRA8; buffer->width = wire->width; buffer->height = wire->height; buffer->plane_count = 1;
    buffer->planes[0].length_bytes = stride * wire->height; buffer->planes[0].stride_bytes = (uint32_t)stride;
    frame->struct_size = sizeof(*frame); frame->abi_version = NXB_MEDIA_ABI_V1; frame->epoch = *expected;
    frame->sequence = wire->sequence; frame->pts_ns = wire->pts_ns; frame->duration_ns = wire->duration_ns; frame->buffer_id = wire->buffer_id;
    frame->pixel_format = NXB_VIDEO_BGRA8; frame->width = wire->width; frame->height = wire->height;
    frame->color_primaries = NXB_COLOR_BT709; frame->transfer = NXB_TRANSFER_BT709; frame->matrix = NXB_MATRIX_RGB;
    frame->color_range = NXB_RANGE_FULL; frame->plane_count = 1; frame->ready_kind = NXB_READY_GPU_COMPLETE;
    return 1;
}

/* Inline PCM is copied into a worker-owned, registered buffer before use;
 * its ID is local to the epoch and is not a Mach port or an IPC pointer. */
static inline int nxb_mach_audio_descriptors(const uint8_t *wire, const nxb_media_epoch_v1 *expected,
        uint64_t local_buffer_id, uint64_t allocation, nxb_audio_buffer_v1 *buffer, nxb_audio_chunk_v1 *chunk) {
    if (!wire || !expected || !buffer || !chunk || memcmp(wire, expected->session_id, 16) ||
        !expected->generation || nxb_wire_get64(wire + 16) != expected->generation || !local_buffer_id ||
        !nxb_wire_get64(wire + 24) || !nxb_wire_get64(wire + 40)) return 0;
    unsigned nonzero = 0; for (unsigned i = 0; i < 16; i++) nonzero |= expected->session_id[i];
    uint32_t frames = nxb_wire_get32(wire + 48), flags = nxb_wire_get32(wire + 52);
    uint64_t first_sample = nxb_wire_get64(wire + 32);
    if (!nonzero || !frames || frames > NXB_AUDIO_FRAMES_MAX || flags & ~NXB_AUDIO_DISCONTINUITY ||
        first_sample > UINT64_MAX - frames || allocation < (uint64_t)frames * NXB_AUDIO_CHANNELS * 4u) return 0;
    *buffer = (nxb_audio_buffer_v1){0}; *chunk = (nxb_audio_chunk_v1){0};
    buffer->struct_size = sizeof(*buffer); buffer->abi_version = NXB_MEDIA_ABI_V1; buffer->epoch = *expected;
    buffer->buffer_id = local_buffer_id; buffer->allocation_bytes = allocation; buffer->storage_kind = NXB_STORAGE_CPU_SHARED;
    chunk->struct_size = sizeof(*chunk); chunk->abi_version = NXB_MEDIA_ABI_V1; chunk->epoch = *expected;
    chunk->sequence = nxb_wire_get64(wire + 24); chunk->first_sample = first_sample;
    chunk->pts_ns = nxb_wire_get64(wire + 40); chunk->buffer_id = local_buffer_id;
    chunk->sample_format = NXB_AUDIO_F32LE_INTERLEAVED; chunk->sample_rate = NXB_AUDIO_RATE;
    chunk->channels = NXB_AUDIO_CHANNELS; chunk->frames = frames; chunk->flags = flags;
    return 1;
}
#endif
