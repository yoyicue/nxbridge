#include "../adapters/nxnative/mach_media.h"
#include "../adapters/nxnative/mach_abi.h"
#include <assert.h>

int main(void) {
    nxb_mach_frame frame = { .session_id = {1, 2, 3}, .generation = 0x0102030405060708ull,
        .sequence = 21, .pts_ns = 1000000000, .duration_ns = 33333333, .buffer_id = 6, .width = 1280, .height = 720 };
    uint8_t bytes[NXB_MACH_PAYLOAD] = {0};
    nxb_mach_pack_frame(bytes, &frame);
    assert(bytes[16] == 8 && bytes[23] == 1 && bytes[56] == 0 && bytes[57] == 5);
    nxb_mach_frame copy = {0}; nxb_mach_unpack_frame(&copy, bytes);
    assert(!memcmp(frame.session_id, copy.session_id, 16));
    assert(copy.generation == frame.generation && copy.sequence == frame.sequence && copy.pts_ns == frame.pts_ns);
    assert(copy.duration_ns == frame.duration_ns && copy.buffer_id == frame.buffer_id && copy.width == 1280 && copy.height == 720);
    uint8_t release[44] = {0}; nxb_mach_pack_release(release, &frame, NXB_MACH_DROPPED);
    assert(!memcmp(release, frame.session_id, 16) && nxb_wire_get64(release + 16) == frame.generation);
    assert(nxb_wire_get64(release + 24) == frame.sequence && nxb_wire_get64(release + 32) == frame.buffer_id);
    assert(nxb_wire_get32(release + 40) == NXB_MACH_DROPPED);
    nxb_media_epoch_v1 epoch = { .session_id = {1, 2, 3}, .generation = frame.generation };
    nxb_video_buffer_v1 buffer; nxb_video_frame_v1 descriptor;
    assert(nxb_mach_video_descriptors(&copy, &epoch, 1280u * 720u * 4u, 1280u * 4u, NXB_COLOR_BT709, &buffer, &descriptor));
    assert(buffer.storage_kind == NXB_STORAGE_IOSURFACE && buffer.buffer_id == frame.buffer_id);
    assert(buffer.planes[0].length_bytes == 1280u * 720u * 4u && descriptor.ready_kind == NXB_READY_GPU_COMPLETE);
    assert(!nxb_mach_video_descriptors(&copy, &epoch, 1280u * 720u * 4u - 1, 1280u * 4u, NXB_COLOR_BT709, &buffer, &descriptor));
    assert(!nxb_mach_video_descriptors(&copy, &epoch, 1280u * 720u * 4u, 1279u * 4u, NXB_COLOR_BT709, &buffer, &descriptor));
    assert(!nxb_mach_video_descriptors(&copy, &epoch, 1280u * 720u * 4u, 1280u * 4u, 0, &buffer, &descriptor));
    epoch.generation++;
    assert(!nxb_mach_video_descriptors(&copy, &epoch, 1280u * 720u * 4u, 1280u * 4u, NXB_COLOR_BT709, &buffer, &descriptor));
    epoch.generation--;
    uint8_t audio[56] = {0}; memcpy(audio, epoch.session_id, 16);
    nxb_wire_u64(audio + 16, epoch.generation); nxb_wire_u64(audio + 24, 4);
    nxb_wire_u64(audio + 32, 240); nxb_wire_u64(audio + 40, 1000000000);
    nxb_wire_u32(audio + 48, 240); nxb_wire_u32(audio + 52, NXB_AUDIO_DISCONTINUITY);
    nxb_audio_buffer_v1 ab; nxb_audio_chunk_v1 chunk;
    assert(nxb_mach_audio_descriptors(audio, &epoch, 1, 3840, &ab, &chunk));
    assert(ab.storage_kind == NXB_STORAGE_CPU_SHARED && chunk.first_sample == 240 && chunk.frames == 240);
    assert(chunk.flags == NXB_AUDIO_DISCONTINUITY && chunk.sample_rate == 48000 && chunk.channels == 2);
    assert(!nxb_mach_audio_descriptors(audio, &epoch, 1, 100, &ab, &chunk));
    nxb_wire_u32(audio + 52, 2);
    assert(!nxb_mach_audio_descriptors(audio, &epoch, 1, 3840, &ab, &chunk));
    nxb_wire_u32(audio + 52, 0); nxb_wire_u64(audio + 32, UINT64_MAX - 100);
    assert(!nxb_mach_audio_descriptors(audio, &epoch, 1, 3840, &ab, &chunk));
    return 0;
}
