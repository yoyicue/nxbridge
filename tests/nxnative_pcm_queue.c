#include "../adapters/nxnative/pcm_queue.h"
#include <assert.h>
#include <pthread.h>
#include <stdint.h>

static nxb_pcm_queue concurrent_q;
static _Atomic int done;
static void *produce(void *unused) {
    (void)unused;
    int16_t s[480] = {1000, -1000};
    for (int i = 0; i < 50000; i++) assert(nxb_pcm_queue_push_s16(&concurrent_q, s, 240) >= 0);
    atomic_store_explicit(&done, 1, memory_order_release);
    return NULL;
}

int main(void) {
    nxb_pcm_queue q;
    nxb_pcm_block out;
    int16_t s[480] = {INT16_MIN, INT16_MAX}; /* 240 stereo frames = 5 ms */
    nxb_pcm_queue_init(&q, 1000000000ull);
    assert(nxb_pcm_queue_push_s16(&q, s, 0) == -1);
    for (int i = 0; i < NXB_PCM_CAPACITY; i++)
        assert(nxb_pcm_queue_push_s16(&q, s, 240) == 1);
    assert(nxb_pcm_queue_push_s16(&q, s, 240) == 0);
    assert(nxb_pcm_queue_pop(&q, &out) == 1);
    assert(out.sequence == 1 && out.first_sample == 0 && out.pts_ns == 1000000000ull);
    assert(out.samples[0] == -1.0f && out.samples[1] > 0.999f && out.flags == 0);
    assert(nxb_pcm_queue_push_s16(&q, s, 240) == 1);
    for (int i = 1; i < NXB_PCM_CAPACITY; i++) assert(nxb_pcm_queue_pop(&q, &out) == 1);
    assert(nxb_pcm_queue_pop(&q, &out) == 1);
    assert(out.sequence == 22 && out.first_sample == 21 * 240 && out.pts_ns == 1105000000ull);
    assert(out.flags == NXB_PCM_DISCONTINUITY);
    assert(nxb_pcm_queue_pop(&q, &out) == 0);
    nxb_pcm_queue_init(&q, 100);
    assert(nxb_pcm_queue_push_s16_at(&q, s, 240, 1000000000ull) == 1);
    assert(nxb_pcm_queue_push_s16_at(&q, s, 240, 1006000000ull) == 1);
    assert(nxb_pcm_queue_push_s16_at(&q, s, 240, 1100000000ull) == 1);
    assert(nxb_pcm_queue_pop(&q, &out) == 1 && out.pts_ns == 1000000000ull && !out.flags);
    assert(nxb_pcm_queue_pop(&q, &out) == 1 && out.pts_ns == 1005000000ull && !out.flags);
    assert(nxb_pcm_queue_pop(&q, &out) == 1 && out.pts_ns == 1100000000ull && out.flags == NXB_PCM_DISCONTINUITY);
    nxb_pcm_queue_init(&q, 1000000000ull);
    assert(nxb_pcm_queue_push_s16_at(&q, s, 1, 1000000000ull) == 1);
    assert(nxb_pcm_queue_push_s16_at(&q, s, 1, 1100000000ull) == 1);
    assert(nxb_pcm_queue_push_s16_at(&q, s, 1, 1100020833ull) == 1);
    assert(nxb_pcm_queue_pop(&q, &out) == 1);
    assert(nxb_pcm_queue_pop(&q, &out) == 1 && out.flags == NXB_PCM_DISCONTINUITY && out.pts_ns == 1100000000ull);
    assert(nxb_pcm_queue_pop(&q, &out) == 1 && out.flags == 0 && out.pts_ns == 1100020833ull);
    nxb_pcm_queue_init(&concurrent_q, 1000000000ull);
    atomic_init(&done, 0);
    pthread_t thread;
    assert(pthread_create(&thread, NULL, produce, NULL) == 0);
    uint64_t previous = 0, received = 0;
    for (;;) {
        if (nxb_pcm_queue_pop(&concurrent_q, &out) == 1) {
            assert(out.sequence > previous);
            assert(out.first_sample == (out.sequence - 1) * 240);
            assert(out.pts_ns == 1000000000ull + (out.sequence - 1) * 5000000ull);
            if (out.sequence != previous + 1) assert(out.flags & NXB_PCM_DISCONTINUITY);
            assert(out.samples[0] > 0 && out.samples[1] < 0);
            previous = out.sequence; received++;
        } else if (atomic_load_explicit(&done, memory_order_acquire)) break;
    }
    assert(pthread_join(thread, NULL) == 0);
    assert(received > 0);
    return 0;
}
