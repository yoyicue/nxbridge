/* Temporary, opt-in diagnostic sink; not a media transport. */
#include "pcm_queue.h"
#include "mach_media.h"
#include <ctype.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
#include <unistd.h>

static nxb_pcm_queue queue;
static _Atomic int enabled;
static int output_fd = -1;
static FILE *trace;
static uint64_t skip_frames;
static mach_port_t worker_port = MACH_PORT_NULL;
static uint8_t session_id[16];
static uint64_t generation;
static int mach_mode;
static pthread_once_t once = PTHREAD_ONCE_INIT;
static void abandon_start(void) {
    if (trace) { fclose(trace); trace = NULL; }
    if (output_fd >= 0) { close(output_fd); output_fd = -1; }
    if (MACH_PORT_VALID(worker_port)) { mach_port_deallocate(mach_task_self(), worker_port); worker_port = MACH_PORT_NULL; }
}

static void *consume(void *unused) {
    (void)unused;
    nxb_pcm_block b;
    uint64_t frames = 0;
    int discontinuity = 0;
    while ((mach_mode || frames < 48000u * 8u) && atomic_load_explicit(&enabled, memory_order_acquire)) {
        int ok = nxb_pcm_queue_pop(&queue, &b);
        if (!ok) { struct timespec ts = {0, 5000000}; nanosleep(&ts, NULL); continue; }
        if (mach_mode) {
            nxb_mach_audio_message msg = {0};
            msg.header.msgh_bits = MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND, 0);
            msg.header.msgh_size = (mach_msg_size_t)(sizeof(mach_msg_header_t) + 56 + b.frames * 8);
            msg.header.msgh_remote_port = worker_port; msg.header.msgh_id = NXB_MACH_AUDIO_ID;
            memcpy(msg.bytes, session_id, 16); nxb_wire_u64(msg.bytes + 16, generation);
            nxb_wire_u64(msg.bytes + 24, b.sequence); nxb_wire_u64(msg.bytes + 32, b.first_sample);
            nxb_wire_u64(msg.bytes + 40, b.pts_ns); nxb_wire_u32(msg.bytes + 48, b.frames);
            nxb_wire_u32(msg.bytes + 52, b.flags | (discontinuity ? NXB_PCM_DISCONTINUITY : 0));
            for (size_t i = 0; i < (size_t)b.frames * 2; i++) { uint32_t bits; memcpy(&bits, &b.samples[i], 4); nxb_wire_u32(msg.bytes + 56 + 4 * i, bits); }
            mach_msg_return_t e = mach_msg(&msg.header, MACH_SEND_MSG | MACH_SEND_TIMEOUT, msg.header.msgh_size, 0, MACH_PORT_NULL, 0, MACH_PORT_NULL);
            discontinuity = e != MACH_MSG_SUCCESS;
            continue;
        }
        size_t bytes = (size_t)b.frames * 2u * sizeof(float), done = 0;
        const char *p = (const char *)b.samples;
        while (done < bytes) {
            ssize_t n = write(output_fd, p + done, bytes - done);
            if (n <= 0) { atomic_store(&enabled, 0); break; }
            done += (size_t)n;
        }
        if (trace) fprintf(trace, "%llu,%llu,%llu,%u,%u\n", (unsigned long long)b.sequence,
                           (unsigned long long)b.first_sample, (unsigned long long)b.pts_ns, b.frames, b.flags);
        frames += b.frames;
    }
    atomic_store_explicit(&enabled, 0, memory_order_release);
    if (output_fd >= 0) close(output_fd);
    if (MACH_PORT_VALID(worker_port)) mach_port_deallocate(mach_task_self(), worker_port);
    if (trace) fclose(trace);
    return NULL;
}

static void begin(void) {
    const char *path = getenv("NXB_PCM_PROBE");
    const char *mode = getenv("NXB_MACH_MEDIA"); mach_mode = mode && !strcmp(mode, "1");
    if ((!path || !*path) && !mach_mode) return;
    if (mach_mode && path && *path) { fprintf(stderr, "[RED] choose one PCM consumer\n"); return; }
    if (mach_mode) {
        const char *sid = getenv("NXB_SESSION_ID"), *gen = getenv("NXB_VIDEO_GENERATION"); char *end = NULL;
        if (!sid || strlen(sid) != 32 || !gen || !*gen) { fprintf(stderr, "[RED] Mach PCM needs session and generation\n"); return; }
        for (const char *p = gen; *p; p++) if (!isdigit((unsigned char)*p)) return;
        errno = 0; generation = strtoull(gen, &end, 10); if (!generation || *end || errno == ERANGE) return;
        unsigned nonzero = 0;
        for (int i = 0; i < 16; i++) {
            if (!isxdigit((unsigned char)sid[i*2]) || !isxdigit((unsigned char)sid[i*2+1])) return;
            char pair[3] = {sid[i*2], sid[i*2+1], 0}; session_id[i] = (uint8_t)strtoul(pair, NULL, 16); nonzero |= session_id[i];
        }
        if (!nonzero) return;
        mach_port_array_t ports = NULL; mach_msg_type_number_t count = 0;
        if (mach_ports_lookup(mach_task_self(), &ports, &count) != KERN_SUCCESS) return;
        if (count > 1) { worker_port = ports[1]; ports[1] = MACH_PORT_NULL; }
        for (mach_msg_type_number_t i = 0; i < count; i++) if (MACH_PORT_VALID(ports[i])) mach_port_deallocate(mach_task_self(), ports[i]);
        vm_deallocate(mach_task_self(), (vm_address_t)ports, count * sizeof(*ports));
        if (!MACH_PORT_VALID(worker_port)) { fprintf(stderr, "[RED] Mach PCM endpoint unavailable\n"); return; }
    }
    const char *skip = getenv("NXB_PCM_PROBE_SKIP_SECONDS");
    if (skip && *skip) {
        char *end = NULL;
        long seconds = strtol(skip, &end, 10);
        if (*end || seconds < 0 || seconds > 120) { fprintf(stderr, "[RED] NXB_PCM_PROBE_SKIP_SECONDS must be 0..120\n"); abandon_start(); return; }
        skip_frames = (uint64_t)seconds * NXB_PCM_RATE;
    }
    if (!mach_mode) {
        output_fd = open(path, O_WRONLY | O_CREAT | O_EXCL, 0600);
        if (output_fd < 0) { fprintf(stderr, "[RED] NXB_PCM_PROBE cannot create output\n"); return; }
    }
    const char *trace_path = getenv("NXB_PCM_TRACE");
    if (trace_path && *trace_path) {
        int fd = open(trace_path, O_WRONLY | O_CREAT | O_EXCL, 0600);
        if (fd < 0 || !(trace = fdopen(fd, "w"))) {
            if (fd >= 0) close(fd);
            abandon_start(); fprintf(stderr, "[RED] NXB_PCM_TRACE cannot create output\n"); return;
        }
        fputs("sequence,first_sample,pts_ns,frames,flags\n", trace);
    }
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts)) { abandon_start(); return; }
    nxb_pcm_queue_init(&queue, (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec);
    atomic_store_explicit(&enabled, 1, memory_order_release);
    pthread_t thread;
    if (pthread_create(&thread, NULL, consume, NULL)) { atomic_store(&enabled, 0); abandon_start(); return; }
    pthread_detach(thread);
}

/* Called before the silent-sink thread starts; output creation is never in pull(). */
void nxb_pcm_probe_start(void) { pthread_once(&once, begin); }

/* Exactly one stereo 48 kHz silent sink may call this. Never waits for disk. */
void nxb_pcm_probe_push(const int16_t *samples, uint32_t frames) {
    if (!atomic_load_explicit(&enabled, memory_order_acquire)) return;
    if (skip_frames >= frames) { skip_frames -= frames; return; }
    samples += skip_frames * NXB_PCM_CHANNELS;
    frames -= (uint32_t)skip_frames;
    skip_frames = 0;
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts)) return;
    uint64_t now = (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec;
    uint64_t duration = (uint64_t)frames * 1000000000ull / NXB_PCM_RATE;
    if (now >= duration) (void)nxb_pcm_queue_push_s16_at(&queue, samples, frames, now - duration);
}
