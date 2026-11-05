/* Bounded Metal -> IOSurface/CVPixelBuffer source. Opt-in test consumer may
 * run VideoToolbox locally, or send IOSurface rights to a Mach worker. */
#import "video_probe.h"
#import <Foundation/Foundation.h>
#import <CoreMedia/CoreMedia.h>
#import <CoreVideo/CoreVideo.h>
#import <VideoToolbox/VideoToolbox.h>
#import <dispatch/dispatch.h>
#import <IOSurface/IOSurface.h>
#include "mach_media.h"
#include <ctype.h>
#include <errno.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

enum { WIDTH = 1280, HEIGHT = 720, SLOTS = 6, ENCODING_MAX = 2 };
enum { FREE, GPU_PENDING, ENCODING };
typedef struct {
    CVPixelBufferRef pixel;
    CVMetalTextureRef cvtexture;
    uint64_t sequence, pts_ns, duration_ns, gpu_done_ns;
    uint64_t register_attempt_ns;
    int state, registration; /* 0 new, 1 pending, 2 acknowledged, 3 rejected */
} slot;
static slot slots[SLOTS];
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t drained = PTHREAD_COND_INITIALIZER;
static pthread_once_t once = PTHREAD_ONCE_INIT;
static CVMetalTextureCacheRef cache;
static VTCompressionSessionRef encoder;
static FILE *output;
static FILE *trace;
static uint64_t trace_rows;
static uint64_t generation, next_sequence, dropped, encoded, failed, written_bytes;
static uint8_t session_id[16];
static char session_hex[33];
static unsigned inflight;
static uint64_t claims;
static dispatch_queue_t submission_queue;
static uint64_t last_submitted;
static unsigned stall_ms;
static _Atomic int active;
static int mach_mode;
static mach_port_t worker_port = MACH_PORT_NULL, release_port = MACH_PORT_NULL;
static pthread_t release_thread;
static int release_thread_started;

static uint64_t now_ns(void) {
    struct timespec ts;
    return clock_gettime(CLOCK_MONOTONIC, &ts) ? 0 : (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec;
}
static void close_files(void) {
    if (output) { fclose(output); output = NULL; }
    if (trace) { fclose(trace); trace = NULL; }
}

static void release_ticket(const uint8_t *bytes) {
    uint64_t gen = nxb_wire_get64(bytes + 16), seq = nxb_wire_get64(bytes + 24), buffer = nxb_wire_get64(bytes + 32);
    uint32_t disposition = nxb_wire_get32(bytes + 40);
    if (disposition < NXB_MACH_ENCODED || disposition > NXB_MACH_FAILED) return;
    pthread_mutex_lock(&lock);
    if (!memcmp(bytes, session_id, sizeof session_id) && gen == generation && buffer >= 1 && buffer <= SLOTS) {
        slot *s = &slots[buffer - 1];
        if (s->state == ENCODING && s->sequence == seq) { s->state = FREE; inflight--; if (disposition == NXB_MACH_ENCODED) encoded++; else if (disposition == NXB_MACH_DROPPED) dropped++; else failed++; pthread_cond_broadcast(&drained); }
    }
    pthread_mutex_unlock(&lock);
}
static void registration_result(const uint8_t *bytes, int accepted) {
    uint64_t gen = nxb_wire_get64(bytes + 16), seq = nxb_wire_get64(bytes + 24), buffer = nxb_wire_get64(bytes + 32);
    pthread_mutex_lock(&lock);
    if (!memcmp(bytes, session_id, sizeof session_id) && gen == generation && !seq && !nxb_wire_get32(bytes + 40) && buffer >= 1 && buffer <= SLOTS) {
        slot *s = &slots[buffer - 1];
        if (s->registration == 1) {
            s->registration = accepted ? 2 : 3;
            if (!accepted) failed++;
            fprintf(stderr, "[nxb] video registration buffer=%llu accepted=%d\n", (unsigned long long)buffer, accepted);
        }
    }
    pthread_mutex_unlock(&lock);
}
static void *receive_releases(void *unused) {
    (void)unused;
    for (;;) {
        nxb_mach_release_message msg = {0};
        mach_msg_return_t result = mach_msg(&msg.header, MACH_RCV_MSG, 0, sizeof msg, release_port, MACH_MSG_TIMEOUT_NONE, MACH_PORT_NULL);
        if (result != MACH_MSG_SUCCESS) break;
        if (msg.header.msgh_bits & MACH_MSGH_BITS_COMPLEX) { mach_msg_destroy(&msg.header); continue; }
        if (msg.header.msgh_size == offsetof(nxb_mach_release_message, trailer)) {
            if (msg.header.msgh_id == NXB_MACH_RELEASE_ID) release_ticket(msg.bytes);
            else if (msg.header.msgh_id == NXB_MACH_REGISTER_ACK_ID) registration_result(msg.bytes, 1);
            else if (msg.header.msgh_id == NXB_MACH_REGISTER_NACK_ID) registration_result(msg.bytes, 0);
        }
    }
    return NULL;
}
static int mach_start(void) {
    mach_port_array_t ports = NULL; mach_msg_type_number_t count = 0;
    if (mach_ports_lookup(mach_task_self(), &ports, &count) != KERN_SUCCESS) return 0;
    if (count > 1) { worker_port = ports[1]; ports[1] = MACH_PORT_NULL; }
    for (mach_msg_type_number_t i = 0; i < count; i++) if (MACH_PORT_VALID(ports[i])) mach_port_deallocate(mach_task_self(), ports[i]);
    vm_deallocate(mach_task_self(), (vm_address_t)ports, count * sizeof(*ports));
    if (!MACH_PORT_VALID(worker_port)) return 0;
    if (mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &release_port) != KERN_SUCCESS) goto fail_worker;
    if (mach_port_insert_right(mach_task_self(), release_port, release_port, MACH_MSG_TYPE_MAKE_SEND) != KERN_SUCCESS) goto fail_receive;
    if (pthread_create(&release_thread, NULL, receive_releases, NULL)) { mach_port_deallocate(mach_task_self(), release_port); goto fail_receive; }
    release_thread_started = 1;
    return 1;
fail_receive:
    mach_port_mod_refs(mach_task_self(), release_port, MACH_PORT_RIGHT_RECEIVE, -1); release_port = MACH_PORT_NULL;
fail_worker:
    mach_port_deallocate(mach_task_self(), worker_port); worker_port = MACH_PORT_NULL;
    return 0;
}
static int mach_send_surface(CVPixelBufferRef pixel, nxb_probe_ticket t, uint64_t pts, uint64_t duration, int message_id) {
    IOSurfaceRef surface = CVPixelBufferGetIOSurface(pixel);
    if (!surface) return 0;
    mach_port_t surface_port = IOSurfaceCreateMachPort(surface);
    if (!MACH_PORT_VALID(surface_port)) return 0;
    nxb_mach_frame_message msg = {0};
    msg.header.msgh_bits = MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND, 0) | MACH_MSGH_BITS_COMPLEX;
    msg.header.msgh_size = (mach_msg_size_t)offsetof(nxb_mach_frame_message, trailer);
    msg.header.msgh_remote_port = worker_port; msg.header.msgh_id = message_id;
    msg.body.msgh_descriptor_count = 2;
    msg.surface.name = surface_port; msg.surface.disposition = MACH_MSG_TYPE_COPY_SEND; msg.surface.type = MACH_MSG_PORT_DESCRIPTOR;
    msg.release.name = release_port; msg.release.disposition = MACH_MSG_TYPE_COPY_SEND; msg.release.type = MACH_MSG_PORT_DESCRIPTOR;
    nxb_mach_frame frame = {0}; memcpy(frame.session_id, t.session_id, 16);
    frame.generation = t.generation; frame.sequence = t.sequence; frame.pts_ns = pts; frame.duration_ns = duration;
    frame.buffer_id = t.slot + 1; frame.width = WIDTH; frame.height = HEIGHT;
    nxb_mach_pack_frame(msg.bytes, &frame);
    mach_msg_return_t result = mach_msg(&msg.header, MACH_SEND_MSG | MACH_SEND_TIMEOUT, msg.header.msgh_size, 0, MACH_PORT_NULL, 0, MACH_PORT_NULL);
    mach_port_deallocate(mach_task_self(), surface_port);
    return result == MACH_MSG_SUCCESS;
}

/* Callback holds its lease while writing. Even a stalled writer is bounded by
 * ENCODING_MAX, and never holds lock while doing I/O. */
static void encoded_frame(void *ctx, void *frame_ctx, OSStatus status,
                          VTEncodeInfoFlags flags, CMSampleBufferRef sample) {
    (void)ctx; (void)flags;
    nxb_probe_ticket *t = frame_ctx;
    pthread_mutex_lock(&lock);
    int valid = !memcmp(t->session_id, session_id, sizeof session_id) && t->generation == generation && t->slot < SLOTS && slots[t->slot].state == ENCODING && slots[t->slot].sequence == t->sequence;
    uint64_t pts_ns = valid ? slots[t->slot].pts_ns : 0, gpu_done_ns = valid ? slots[t->slot].gpu_done_ns : 0;
    pthread_mutex_unlock(&lock);
    if (stall_ms) usleep(stall_ms * 1000u);
    int ok = status == noErr && sample && CMSampleBufferDataIsReady(sample);
    if (ok) {
        CMBlockBufferRef b = CMSampleBufferGetDataBuffer(sample);
        size_t n = b ? CMBlockBufferGetDataLength(b) : 0;
        if (n && n <= 16u * 1024u * 1024u) {
            char *bytes = malloc(n);
            if (!bytes || CMBlockBufferCopyDataBytes(b, 0, n, bytes) != noErr) ok = 0;
            CMFormatDescriptionRef fmt = CMSampleBufferGetFormatDescription(sample);
            const uint8_t *sps = NULL, *pps = NULL; size_t sps_len = 0, pps_len = 0, count = 0; int length_bytes = 0;
            if (!fmt || CMVideoFormatDescriptionGetH264ParameterSetAtIndex(fmt, 0, &sps, &sps_len, &count, &length_bytes) != noErr ||
                CMVideoFormatDescriptionGetH264ParameterSetAtIndex(fmt, 1, &pps, &pps_len, NULL, NULL) != noErr || length_bytes != 4) ok = 0;
            if (ok) {
                flockfile(output);
                const unsigned char start[4] = {0, 0, 0, 1};
                if (written_bytes + n + sps_len + pps_len + 8 <= 64u * 1024u * 1024u) {
                    if (fwrite(start, 1, 4, output) != 4 || fwrite(sps, 1, sps_len, output) != sps_len ||
                        fwrite(start, 1, 4, output) != 4 || fwrite(pps, 1, pps_len, output) != pps_len) ok = 0;
                    size_t offset = 0;
                    while (ok && offset + 4 <= n) {
                        const unsigned char *p = (const unsigned char *)bytes + offset;
                        uint32_t len = ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) | ((uint32_t)p[2] << 8) | p[3];
                        offset += 4;
                        if (!len || len > n - offset || fwrite(start, 1, 4, output) != 4 || fwrite(bytes + offset, 1, len, output) != len) { ok = 0; break; }
                        offset += len;
                    }
                    if (offset != n) ok = 0;
                    if (ok) written_bytes += n + sps_len + pps_len + 8;
                }
                funlockfile(output);
            }
            free(bytes);
        } else ok = 0;
    }
    if (trace && valid) {
        flockfile(trace);
        if (trace_rows++ < 100000) fprintf(trace, "%s,%llu,%llu,%u,%llu,%llu,%llu,%d\n", session_hex, (unsigned long long)t->generation,
            (unsigned long long)t->sequence, t->slot, (unsigned long long)pts_ns, (unsigned long long)gpu_done_ns,
            (unsigned long long)now_ns(), ok);
        funlockfile(trace);
    }
    pthread_mutex_lock(&lock);
    if (!memcmp(t->session_id, session_id, sizeof session_id) && t->generation == generation && t->slot < SLOTS && slots[t->slot].state == ENCODING && slots[t->slot].sequence == t->sequence) {
        slots[t->slot].state = FREE;
        inflight--;
        pthread_cond_broadcast(&drained);
        if (ok) encoded++; else failed++;
        if ((encoded + failed) % 300 == 0)
            fprintf(stderr, "[nxb] video encoded=%llu dropped=%llu failed=%llu inflight=%u\n", (unsigned long long)encoded, (unsigned long long)dropped, (unsigned long long)failed, inflight);
    }
    pthread_mutex_unlock(&lock);
    free(t);
}

static void init(void) {
    const char *path = getenv("NXB_VIDEO_PROBE");
    const char *mach = getenv("NXB_MACH_MEDIA");
    mach_mode = mach && !strcmp(mach, "1");
    if ((!path || !*path) && !mach_mode) return;
    if (mach_mode && path && *path) { fprintf(stderr, "[RED] choose one video consumer\n"); return; }
    if (!mach_mode) {
        output = fopen(path, "wx");
        if (!output) { fprintf(stderr, "[RED] NXB_VIDEO_PROBE cannot create output\n"); return; }
    }
    const char *trace_path = getenv("NXB_VIDEO_TRACE");
    if (trace_path && *trace_path) {
        trace = fopen(trace_path, "wx");
        if (!trace) { fprintf(stderr, "[RED] NXB_VIDEO_TRACE cannot create output\n"); close_files(); return; }
        fputs("session_id,generation,sequence,slot,pts_ns,gpu_done_ns,encoded_ns,ok\n", trace);
    }
    const char *g = getenv("NXB_VIDEO_GENERATION");
    char *end = NULL;
    if (g) for (const char *p = g; *p; p++) if (!isdigit((unsigned char)*p)) { fprintf(stderr, "[RED] invalid video generation\n"); close_files(); return; }
    errno = 0;
    generation = g ? strtoull(g, &end, 10) : 1;
    if (!generation || errno == ERANGE || (g && (!*g || *end))) { fprintf(stderr, "[RED] invalid video generation\n"); close_files(); return; }
    const char *sid = getenv("NXB_SESSION_ID");
    if (sid) {
        if (strlen(sid) != 32) { fprintf(stderr, "[RED] invalid session id\n"); close_files(); return; }
        for (unsigned i = 0; i < 16; i++) {
            if (!isxdigit((unsigned char)sid[i * 2]) || !isxdigit((unsigned char)sid[i * 2 + 1])) { fprintf(stderr, "[RED] invalid session id\n"); close_files(); return; }
            char pair[3] = {sid[i * 2], sid[i * 2 + 1], 0};
            char *stop = NULL; unsigned long x = strtoul(pair, &stop, 16);
            if (*stop || stop != pair + 2) { fprintf(stderr, "[RED] invalid session id\n"); close_files(); return; }
            session_id[i] = (uint8_t)x;
        }
    } else arc4random_buf(session_id, sizeof session_id);
    unsigned nonzero = 0; for (unsigned i = 0; i < 16; i++) nonzero |= session_id[i];
    if (!nonzero) { fprintf(stderr, "[RED] zero session id\n"); close_files(); return; }
    for (unsigned i = 0; i < 16; i++) snprintf(session_hex + 2 * i, 3, "%02x", session_id[i]);
    if (mach_mode) {
        if (!mach_start()) { fprintf(stderr, "[RED] cannot initialize Mach media source\n"); close_files(); return; }
        atomic_store_explicit(&active, 1, memory_order_release);
        fprintf(stderr, "[nxb] Mach media source session=%s generation=%llu\n", session_hex, (unsigned long long)generation);
        return;
    }
    const char *stall = getenv("NXB_VIDEO_PROBE_STALL_MS");
    if (stall && *stall) {
        long n = strtol(stall, &end, 10);
        if (*end || n < 0 || n > 5000) { fprintf(stderr, "[RED] invalid video probe stall\n"); close_files(); return; }
        stall_ms = (unsigned)n;
    }
    submission_queue = dispatch_queue_create("org.nxbridge.video-probe-submit", DISPATCH_QUEUE_SERIAL);
    if (!submission_queue) { close_files(); return; }
    OSStatus e = VTCompressionSessionCreate(NULL, WIDTH, HEIGHT, kCMVideoCodecType_H264, NULL, NULL, NULL, encoded_frame, NULL, &encoder);
    if (e != noErr) { fprintf(stderr, "[RED] VideoToolbox session creation: %d\n", (int)e); close_files(); return; }
    VTSessionSetProperty(encoder, kVTCompressionPropertyKey_RealTime, kCFBooleanTrue);
    VTSessionSetProperty(encoder, kVTCompressionPropertyKey_AllowFrameReordering, kCFBooleanFalse);
    int bitrate = 5000000; CFNumberRef n = CFNumberCreate(NULL, kCFNumberIntType, &bitrate);
    VTSessionSetProperty(encoder, kVTCompressionPropertyKey_AverageBitRate, n); CFRelease(n);
    if (VTCompressionSessionPrepareToEncodeFrames(encoder) != noErr) { fprintf(stderr, "[RED] VideoToolbox prepare failed\n"); CFRelease(encoder); encoder = NULL; close_files(); return; }
    CFTypeRef hardware = NULL;
    OSStatus hw_status = VTSessionCopyProperty(encoder, kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder, NULL, &hardware);
    fprintf(stderr, "[nxb] video hardware encoder=%s\n", hw_status == noErr && hardware && CFGetTypeID(hardware) == CFBooleanGetTypeID() ?
            (CFBooleanGetValue((CFBooleanRef)hardware) ? "yes" : "no") : "unknown");
    if (hardware) CFRelease(hardware);
    atomic_store_explicit(&active, 1, memory_order_release);
    fprintf(stderr, "[nxb] video probe generation=%llu, %dx%d, max GPU/encode slots=%d\n", (unsigned long long)generation, WIDTH, HEIGHT, SLOTS);
}

id<MTLTexture> nxb_video_probe_claim(id<MTLDevice> device, double duration, nxb_probe_ticket *t) {
    pthread_once(&once, init);
    if (!atomic_load_explicit(&active, memory_order_acquire) || !t || !device) return nil;
    pthread_mutex_lock(&lock);
    if (!atomic_load_explicit(&active, memory_order_relaxed)) { pthread_mutex_unlock(&lock); return nil; }
    /* ABI v1 reserves at most one producer-writing buffer. Never wait for
     * GPU completion on the game/present thread when the slot is occupied. */
    for (unsigned i = 0; i < SLOTS; i++) if (slots[i].state == GPU_PENDING) { dropped++; pthread_mutex_unlock(&lock); return nil; }
    unsigned k;
    for (k = 0; k < SLOTS && slots[k].state != FREE; k++) {}
    if (k == SLOTS) { dropped++; pthread_mutex_unlock(&lock); return nil; }
    if (!cache && CVMetalTextureCacheCreate(NULL, NULL, device, NULL, &cache) != kCVReturnSuccess) { failed++; pthread_mutex_unlock(&lock); return nil; }
    slot *s = &slots[k];
    if (!s->pixel) {
        NSDictionary *attrs = @{(__bridge NSString *)kCVPixelBufferIOSurfacePropertiesKey: @{}, (__bridge NSString *)kCVPixelBufferMetalCompatibilityKey: @YES};
        CVReturn r = CVPixelBufferCreate(NULL, WIDTH, HEIGHT, kCVPixelFormatType_32BGRA, (__bridge CFDictionaryRef)attrs, &s->pixel);
        NSDictionary *texture_attrs = @{(__bridge NSString *)kCVMetalTextureUsage: @(MTLTextureUsageRenderTarget | MTLTextureUsageShaderRead)};
        if (r == kCVReturnSuccess) r = CVMetalTextureCacheCreateTextureFromImage(NULL, cache, s->pixel, (__bridge CFDictionaryRef)texture_attrs, MTLPixelFormatBGRA8Unorm, WIDTH, HEIGHT, 0, &s->cvtexture);
        if (r != kCVReturnSuccess || !s->cvtexture) {
            if (s->cvtexture) CFRelease(s->cvtexture);
            if (s->pixel) CFRelease(s->pixel);
            s->cvtexture = NULL; s->pixel = NULL; failed++; pthread_mutex_unlock(&lock); return nil;
        }
    }
    uint64_t stamp = now_ns();
    if (!stamp) { failed++; pthread_mutex_unlock(&lock); return nil; }
    if (mach_mode && s->registration != 2) {
        if (s->registration != 3 && (s->registration == 0 || stamp - s->register_attempt_ns >= 1000000000ull)) {
            nxb_probe_ticket registration = {0}; memcpy(registration.session_id, session_id, sizeof session_id);
            registration.generation = generation; registration.slot = k;
            s->registration = 1; s->register_attempt_ns = stamp;
            if (!mach_send_surface(s->pixel, registration, 0, 0, NXB_MACH_REGISTER_ID)) s->registration = 0;
        }
        dropped++; pthread_mutex_unlock(&lock); return nil;
    }
    s->state = GPU_PENDING; s->sequence = ++next_sequence; s->pts_ns = stamp;
    if (++claims % 300 == 0) {
        unsigned occupied = 0; for (unsigned i = 0; i < SLOTS; i++) occupied += slots[i].state != FREE;
        fprintf(stderr, "[nxb] video claims=%llu occupied=%u inflight=%u dropped=%llu failed=%llu\n", (unsigned long long)claims, occupied, inflight, (unsigned long long)dropped, (unsigned long long)failed);
    }
    s->duration_ns = duration > 0 && duration < 1 ? (uint64_t)(duration * 1e9) : 33333333;
    memcpy(t->session_id, session_id, sizeof session_id);
    t->sequence = s->sequence; t->generation = generation; t->slot = k;
    id<MTLTexture> texture = CVMetalTextureGetTexture(s->cvtexture);
    pthread_mutex_unlock(&lock);
    return texture;
}

void nxb_video_probe_complete(nxb_probe_ticket t, int gpu_ok) {
    pthread_mutex_lock(&lock);
    if (memcmp(t.session_id, session_id, sizeof session_id) || t.generation != generation || t.slot >= SLOTS || slots[t.slot].state != GPU_PENDING || slots[t.slot].sequence != t.sequence) { pthread_mutex_unlock(&lock); return; }
    slot *s = &slots[t.slot];
    /* A sixth lease may be in transit while the worker returns an evicted
     * pending frame; the worker itself still caps pending=3 and encoding=2. */
    if (!gpu_ok || !atomic_load_explicit(&active, memory_order_relaxed) || inflight >= (mach_mode ? SLOTS : ENCODING_MAX)) {
        s->state = FREE; if (gpu_ok) dropped++; else failed++;
        pthread_cond_broadcast(&drained); pthread_mutex_unlock(&lock); return;
    }
    if (mach_mode) {
        s->state = ENCODING; s->gpu_done_ns = now_ns(); inflight++;
        CVPixelBufferRef pixel = s->pixel; uint64_t pts = s->pts_ns, duration = s->duration_ns;
        pthread_mutex_unlock(&lock);
        if (!mach_send_surface(pixel, t, pts, duration, NXB_MACH_FRAME_ID)) {
            nxb_mach_frame f = {0}; memcpy(f.session_id, t.session_id, 16); f.generation = t.generation; f.sequence = t.sequence; f.buffer_id = t.slot + 1;
            uint8_t bytes[44]; nxb_mach_pack_release(bytes, &f, NXB_MACH_DROPPED); release_ticket(bytes);
        }
        return;
    }
    nxb_probe_ticket *copy = malloc(sizeof *copy);
    if (!copy) { s->state = FREE; failed++; pthread_cond_broadcast(&drained); pthread_mutex_unlock(&lock); return; }
    *copy = t; s->state = ENCODING; s->gpu_done_ns = now_ns(); inflight++;
    CMTime pts = CMTimeMake((int64_t)s->pts_ns, 1000000000), duration = CMTimeMake((int64_t)s->duration_ns, 1000000000);
    CVPixelBufferRef pixel = s->pixel;
    /* Metal completion handlers may run concurrently. Submit serially and
     * reject out-of-order callbacks to keep VideoToolbox timestamps monotonic.
     * Enqueue while locked so shutdown cannot overtake this submission. */
    dispatch_async(submission_queue, ^{
        if (copy->sequence <= last_submitted) { encoded_frame(NULL, copy, -1, 0, NULL); return; }
        last_submitted = copy->sequence;
        OSStatus e = VTCompressionSessionEncodeFrame(encoder, pixel, pts, duration, NULL, copy, NULL);
        if (e != noErr) encoded_frame(NULL, copy, e, 0, NULL);
    });
    pthread_mutex_unlock(&lock);
}

void nxb_video_probe_shutdown(void) {
    if (!atomic_exchange_explicit(&active, 0, memory_order_acq_rel)) return;
    pthread_mutex_lock(&lock);
    for (;;) {
        int gpu_pending = 0;
        for (unsigned i = 0; i < SLOTS; i++) gpu_pending += slots[i].state == GPU_PENDING;
        if (!gpu_pending) break;
        pthread_cond_wait(&drained, &lock);
    }
    pthread_mutex_unlock(&lock);
    if (mach_mode) {
        struct timespec deadline; clock_gettime(CLOCK_REALTIME, &deadline); deadline.tv_sec++;
        pthread_mutex_lock(&lock);
        while (inflight && pthread_cond_timedwait(&drained, &lock, &deadline) == 0) {}
        unsigned remaining = inflight;
        fprintf(stderr, "[nxb] Mach video shutdown generation=%llu claims=%llu encoded=%llu dropped=%llu failed=%llu outstanding=%u\n",
                (unsigned long long)generation, (unsigned long long)claims, (unsigned long long)encoded, (unsigned long long)dropped, (unsigned long long)failed, remaining);
        pthread_mutex_unlock(&lock);
        if (remaining) return; /* Isolate unreleased buffers until the process exits. */
        if (MACH_PORT_VALID(release_port)) {
            mach_port_deallocate(mach_task_self(), release_port);
            mach_port_mod_refs(mach_task_self(), release_port, MACH_PORT_RIGHT_RECEIVE, -1);
        }
        if (release_thread_started) pthread_join(release_thread, NULL);
        if (MACH_PORT_VALID(worker_port)) mach_port_deallocate(mach_task_self(), worker_port);
        for (unsigned i = 0; i < SLOTS; i++) { if (slots[i].cvtexture) CFRelease(slots[i].cvtexture); if (slots[i].pixel) CFRelease(slots[i].pixel); }
        if (cache) CFRelease(cache);
        close_files();
        return;
    }
    dispatch_sync(submission_queue, ^{});
    OSStatus result = VTCompressionSessionCompleteFrames(encoder, kCMTimeInvalid);
    pthread_mutex_lock(&lock);
    while (inflight) pthread_cond_wait(&drained, &lock);
    fprintf(stderr, "[nxb] video shutdown generation=%llu claims=%llu encoded=%llu dropped=%llu failed=%llu inflight=%u complete=%d\n",
            (unsigned long long)generation, (unsigned long long)claims, (unsigned long long)encoded,
            (unsigned long long)dropped, (unsigned long long)failed, inflight, (int)result);
    pthread_mutex_unlock(&lock);
    VTCompressionSessionInvalidate(encoder); CFRelease(encoder); encoder = NULL;
    for (unsigned i = 0; i < SLOTS; i++) {
        if (slots[i].cvtexture) CFRelease(slots[i].cvtexture);
        if (slots[i].pixel) CFRelease(slots[i].pixel);
    }
    if (cache) CFRelease(cache);
    close_files();
}
