/* NXBridge media descriptor ABI v1. In-process native C layout, not an IPC wire format. */
#ifndef NXBRIDGE_MEDIA_H
#define NXBRIDGE_MEDIA_H
#include <stdint.h>

#define NXB_MEDIA_ABI_V1 1u
#define NXB_VIDEO_BGRA8 1u
#define NXB_VIDEO_NV12 2u
#define NXB_COLOR_BT709 1u
#define NXB_TRANSFER_BT709 1u
#define NXB_MATRIX_RGB 1u
#define NXB_MATRIX_BT709 2u
#define NXB_RANGE_FULL 1u
#define NXB_RANGE_VIDEO 2u
#define NXB_STORAGE_CPU_SHARED 1u
#define NXB_STORAGE_IOSURFACE 2u
#define NXB_READY_CPU_COMPLETE 1u
#define NXB_READY_GPU_COMPLETE 2u
#define NXB_AUDIO_F32LE_INTERLEAVED 1u
#define NXB_VIDEO_PENDING_MAX 3u
#define NXB_VIDEO_INFLIGHT_MAX 2u
#define NXB_VIDEO_BUFFERS_MAX 6u
#define NXB_AUDIO_CHUNKS_MAX 20u
#define NXB_AUDIO_INFLIGHT_MAX 1u
#define NXB_AUDIO_BUFFERS_MAX 22u
#define NXB_AUDIO_DISCONTINUITY 1u
#define NXB_AUDIO_RATE 48000u
#define NXB_AUDIO_CHANNELS 2u
#define NXB_AUDIO_FRAMES_MAX 480u

typedef struct nxb_media_epoch_v1 {
    uint8_t session_id[16];
    uint64_t generation;
} nxb_media_epoch_v1;

typedef struct nxb_plane_v1 {
    uint64_t offset_bytes;
    uint64_t length_bytes;
    uint32_t stride_bytes;
    uint32_t reserved;
} nxb_plane_v1;

/* A registered buffer ID resolves only within this epoch. Transfer the actual
 * OS handle out of band; never copy a pointer or Mach port name as an integer. */
typedef struct nxb_video_buffer_v1 {
    uint32_t struct_size;
    uint32_t abi_version;
    nxb_media_epoch_v1 epoch;
    uint64_t buffer_id;
    uint64_t allocation_bytes;
    uint32_t storage_kind;
    uint32_t pixel_format;
    uint32_t width;
    uint32_t height;
    uint32_t plane_count;
    uint32_t reserved;
    nxb_plane_v1 planes[2];
} nxb_video_buffer_v1;

typedef struct nxb_video_frame_v1 {
    uint32_t struct_size;
    uint32_t abi_version;
    nxb_media_epoch_v1 epoch;
    uint64_t sequence;
    uint64_t pts_ns;
    uint64_t duration_ns;
    uint64_t buffer_id;
    uint32_t pixel_format;
    uint32_t width;
    uint32_t height;
    uint32_t color_primaries;
    uint32_t transfer;
    uint32_t matrix;
    uint32_t color_range;
    uint32_t plane_count;
    uint32_t ready_kind;
    uint32_t reserved[3];
} nxb_video_frame_v1;

typedef struct nxb_audio_buffer_v1 {
    uint32_t struct_size;
    uint32_t abi_version;
    nxb_media_epoch_v1 epoch;
    uint64_t buffer_id;
    uint64_t allocation_bytes;
    uint32_t storage_kind;
    uint32_t reserved;
} nxb_audio_buffer_v1;

typedef struct nxb_audio_chunk_v1 {
    uint32_t struct_size;
    uint32_t abi_version;
    nxb_media_epoch_v1 epoch;
    uint64_t sequence;
    uint64_t pts_ns;
    uint64_t first_sample;
    uint64_t buffer_id;
    uint32_t sample_format;
    uint32_t sample_rate;
    uint32_t channels;
    uint32_t frames;
    uint32_t flags;
    uint32_t reserved;
} nxb_audio_chunk_v1;

typedef struct nxb_video_release_v1 {
    uint32_t struct_size;
    uint32_t abi_version;
    nxb_media_epoch_v1 epoch;
    uint64_t sequence;
    uint64_t buffer_id;
    uint32_t disposition;
    uint32_t reserved;
} nxb_video_release_v1;
#define NXB_RELEASE_ENCODED 1u
#define NXB_RELEASE_DROPPED 2u
#define NXB_RELEASE_FAILED 3u
typedef nxb_video_release_v1 nxb_audio_release_v1;
#endif
