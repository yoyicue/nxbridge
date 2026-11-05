#ifndef NXBRIDGE_VIDEO_PROBE_H
#define NXBRIDGE_VIDEO_PROBE_H
#import <Metal/Metal.h>
#include <stdint.h>

typedef struct { uint8_t session_id[16]; uint64_t sequence, generation; unsigned slot; } nxb_probe_ticket;
/* Each claim returns a distinct lease until GPU completion and encoding release.
 * Both functions are no-ops unless NXB_VIDEO_PROBE names a new output file. */
id<MTLTexture> nxb_video_probe_claim(id<MTLDevice> device, double duration, nxb_probe_ticket *ticket);
void nxb_video_probe_complete(nxb_probe_ticket ticket, int gpu_ok);
/* Call after the source has stopped producing and drained its GPU queue. */
void nxb_video_probe_shutdown(void);
#endif
