/* Single-producer/single-consumer staging queue for the nxnative audio sink.
 * This is not the cross-process media ABI or a buffer lease. */
#ifndef NXBRIDGE_PCM_QUEUE_H
#define NXBRIDGE_PCM_QUEUE_H
#include <stdatomic.h>
#include <stdint.h>

enum { NXB_PCM_RATE = 48000, NXB_PCM_CHANNELS = 2, NXB_PCM_MAX_FRAMES = 480, NXB_PCM_CAPACITY = 20 };
enum { NXB_PCM_DISCONTINUITY = 1 };

typedef struct {
    uint64_t sequence, first_sample, pts_ns;
    uint32_t frames, flags;
    float samples[NXB_PCM_MAX_FRAMES * NXB_PCM_CHANNELS];
} nxb_pcm_block;

typedef struct {
    _Atomic uint64_t read_pos, write_pos;
    uint64_t anchor_ns, anchor_sample, next_sample, next_sequence;
    int discontinuity;
    nxb_pcm_block blocks[NXB_PCM_CAPACITY];
} nxb_pcm_queue;

/* Initialize before publishing the queue to either thread; never reset while in use. */
void nxb_pcm_queue_init(nxb_pcm_queue *q, uint64_t anchor_ns);
/* One producer; 1 queued, 0 dropped due to backpressure, -1 invalid input.
 * A valid dropped block still advances the audio sample clock. */
int nxb_pcm_queue_push_s16(nxb_pcm_queue *q, const int16_t *samples, uint32_t frames);
/* Producer clock observation for the first sample, CLOCK_MONOTONIC ns. A
 * late wakeup over 20 ms reanchors and marks the next block discontinuous. */
int nxb_pcm_queue_push_s16_at(nxb_pcm_queue *q, const int16_t *samples, uint32_t frames, uint64_t first_sample_ns);
/* One consumer; copies the block before releasing the staging slot. */
int nxb_pcm_queue_pop(nxb_pcm_queue *q, nxb_pcm_block *out);
#endif
