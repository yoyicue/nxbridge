#include "pcm_queue.h"
#include <stddef.h>

void nxb_pcm_queue_init(nxb_pcm_queue *q, uint64_t anchor_ns) {
    atomic_init(&q->read_pos, 0);
    atomic_init(&q->write_pos, 0);
    q->anchor_ns = anchor_ns;
    q->anchor_sample = 0;
    q->next_sample = 0;
    q->next_sequence = 1;
    q->discontinuity = 0;
}

int nxb_pcm_queue_push_s16(nxb_pcm_queue *q, const int16_t *samples, uint32_t frames) {
    if (!q || !samples || !frames || frames > NXB_PCM_MAX_FRAMES ||
        q->next_sample > UINT64_MAX - frames || q->next_sequence == UINT64_MAX)
        return -1;
    uint64_t first = q->next_sample, sequence = q->next_sequence++;
    uint64_t delta = first - q->anchor_sample;
    uint64_t seconds = delta / NXB_PCM_RATE;
    uint64_t fraction = (delta % NXB_PCM_RATE) * 1000000000ull / NXB_PCM_RATE;
    if (seconds > (UINT64_MAX - fraction) / 1000000000ull ||
        q->anchor_ns > UINT64_MAX - (seconds * 1000000000ull + fraction)) {
        q->next_sequence--;
        return -1;
    }
    q->next_sample += frames;
    uint64_t w = atomic_load_explicit(&q->write_pos, memory_order_relaxed);
    uint64_t r = atomic_load_explicit(&q->read_pos, memory_order_acquire);
    if (w - r >= NXB_PCM_CAPACITY) { q->discontinuity = 1; return 0; }
    nxb_pcm_block *b = &q->blocks[w % NXB_PCM_CAPACITY];
    b->sequence = sequence; b->first_sample = first;
    b->pts_ns = q->anchor_ns + seconds * 1000000000ull + fraction;
    b->frames = frames; b->flags = q->discontinuity ? NXB_PCM_DISCONTINUITY : 0;
    q->discontinuity = 0;
    for (size_t i = 0; i < (size_t)frames * NXB_PCM_CHANNELS; i++)
        b->samples[i] = (float)samples[i] / 32768.0f;
    atomic_store_explicit(&q->write_pos, w + 1, memory_order_release);
    return 1;
}

int nxb_pcm_queue_push_s16_at(nxb_pcm_queue *q, const int16_t *samples, uint32_t frames, uint64_t observed) {
    if (!q || !samples || !frames || frames > NXB_PCM_MAX_FRAMES || !observed ||
        q->next_sample > UINT64_MAX - frames || q->next_sequence == UINT64_MAX) return -1;
    uint64_t delta = q->next_sample - q->anchor_sample;
    uint64_t seconds = delta / NXB_PCM_RATE;
    uint64_t fraction = (delta % NXB_PCM_RATE) * 1000000000ull / NXB_PCM_RATE;
    if (seconds > (UINT64_MAX - fraction) / 1000000000ull) return -1;
    uint64_t elapsed = seconds * 1000000000ull + fraction;
    if (q->anchor_ns > UINT64_MAX - elapsed) return -1;
    uint64_t expected = q->anchor_ns + elapsed;
    if (!q->next_sample || (observed > expected && observed - expected > 20000000ull)) {
        q->anchor_ns = observed;
        q->anchor_sample = q->next_sample;
        if (q->next_sample) q->discontinuity = 1;
    }
    return nxb_pcm_queue_push_s16(q, samples, frames);
}

int nxb_pcm_queue_pop(nxb_pcm_queue *q, nxb_pcm_block *out) {
    if (!q || !out) return -1;
    uint64_t r = atomic_load_explicit(&q->read_pos, memory_order_relaxed);
    uint64_t w = atomic_load_explicit(&q->write_pos, memory_order_acquire);
    if (r == w) return 0;
    *out = q->blocks[r % NXB_PCM_CAPACITY];
    atomic_store_explicit(&q->read_pos, r + 1, memory_order_release);
    return 1;
}
