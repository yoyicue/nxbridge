#include "nxbridge_media.h"
#include <stddef.h>
_Static_assert(sizeof(nxb_media_epoch_v1) == 24, "epoch size");
_Static_assert(sizeof(nxb_plane_v1) == 24, "plane size");
_Static_assert(sizeof(nxb_video_buffer_v1) == 120, "video buffer size");
_Static_assert(sizeof(nxb_video_frame_v1) == 112, "video frame size");
_Static_assert(sizeof(nxb_audio_buffer_v1) == 56, "audio buffer size");
_Static_assert(sizeof(nxb_audio_chunk_v1) == 88, "audio chunk size");
_Static_assert(sizeof(nxb_video_release_v1) == 56, "release size");
_Static_assert(offsetof(nxb_video_buffer_v1, planes) == 72, "plane offset");
_Static_assert(offsetof(nxb_video_frame_v1, sequence) == 32, "sequence offset");
_Static_assert(offsetof(nxb_audio_chunk_v1, first_sample) == 48, "sample offset");
_Static_assert(offsetof(nxb_video_release_v1, disposition) == 48, "disposition offset");
int main(void) { return 0; }
