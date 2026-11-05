/* Experimental single-game worker: inherit a Mach endpoint into a spawned
 * game, import IOSurfaces, encode and return matching leases. */
#import <Foundation/Foundation.h>
#import <CoreMedia/CoreMedia.h>
#import <CoreVideo/CoreVideo.h>
#import <IOSurface/IOSurface.h>
#import <VideoToolbox/VideoToolbox.h>
#import <dispatch/dispatch.h>
#include "../adapters/nxnative/mach_media.h"
#include "../adapters/nxnative/mach_abi.h"
#include <assert.h>
#include <errno.h>
#include <pthread.h>
#include <math.h>
#include <signal.h>
#include <spawn.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <sys/socket.h>
#include <fcntl.h>
#include <time.h>
#include <unistd.h>

extern char **environ;
typedef struct { nxb_mach_frame frame; mach_port_t release; CVPixelBufferRef pixel; uint64_t queued_ns, encode_started_ns; } job;
static VTCompressionSessionRef encoder;
static FILE *output;
static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t drained = PTHREAD_COND_INITIALIZER;
static unsigned inflight;
/* Only raw, not-yet-submitted frames may be replaced. VT reference frames
 * remain ordered through the encoder and the byte-stream pipe. */
static job *pending[1];
#define NXB_RAW_QUEUE_MAX_NS (2ull * 1000000000ull / 30)
static unsigned pending_count;
static int pump_scheduled;
static dispatch_queue_t pump_queue;
static _Atomic uint64_t received, encoded, dropped, failures;
static uint64_t written;
static unsigned stall_ms;
static int bridge_video, bridge_audio_fd = -1;
static uint64_t bridge_audio_dropped;
static uint64_t bridge_audio_full, bridge_audio_other;
static int bridge_audio_discontinuity;
static uint64_t bridge_next_video_pts;
static volatile sig_atomic_t stop_requested;
static void stop_signal(int signum) { (void)signum; stop_requested = 1; }
/* Keep the first imported object for the epoch; a buffer ID cannot silently
 * change its backing surface, and cannot be reused while an encoder owns it. */
static IOSurfaceRef registered[6];
static nxb_video_buffer_v1 registered_buffers[6];
static uint64_t active_sequence[6];
static unsigned registered_count;
static int stale_registration_ack;
static FILE *audio_output;
static FILE *event_trace;
static uint64_t trace_events;
static uint64_t audio_blocks, audio_frames, audio_nonzero, audio_discontinuities, audio_last_seq, audio_last_sample, audio_last_pts;
static uint64_t audio_anchor_sample, audio_anchor_pts;
static float audio_buffer[NXB_MACH_AUDIO_MAX * 2];
static nxb_audio_buffer_v1 audio_registration;
static uint64_t audio_busy_sequence, audio_releases;
static _Atomic uint64_t queue_max_ns, encode_max_ns, pipe_max_ns, raw_expired, raw_replaced;
static uint64_t source_last_pts, source_gap_max_ns, source_gaps_over_100ms;
static uint64_t source_last_receive_ns, source_receive_gap_max_ns;

static uint64_t monotonic_ns(void) {
    struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t);
    return (uint64_t)t.tv_sec * 1000000000ull + (uint64_t)t.tv_nsec;
}
static void atomic_max_ns(_Atomic uint64_t *value, uint64_t sample) {
    uint64_t previous = atomic_load(value);
    while (previous < sample && !atomic_compare_exchange_weak(value, &previous, sample)) {}
}
static void report_video_timing(void) {
    fprintf(stderr, "[nxb-worker] video lifetime max_ms source_pts_gap=%.3f receive_gap=%.3f raw_queue=%.3f vt_callback=%.3f pipe_write=%.3f source_gaps_over_100ms=%llu raw_expired=%llu raw_replaced=%llu\n",
        source_gap_max_ns / 1e6, source_receive_gap_max_ns / 1e6,
        atomic_load(&queue_max_ns) / 1e6, atomic_load(&encode_max_ns) / 1e6, atomic_load(&pipe_max_ns) / 1e6,
        (unsigned long long)source_gaps_over_100ms, (unsigned long long)atomic_load(&raw_expired), (unsigned long long)atomic_load(&raw_replaced));
}

static void reply_frame(mach_port_t port, const nxb_mach_frame *frame, int message_id, uint32_t disposition) {
    nxb_mach_release_message m = {0};
    m.header.msgh_bits = MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND, 0);
    m.header.msgh_size = (mach_msg_size_t)offsetof(nxb_mach_release_message, trailer);
    m.header.msgh_remote_port = port; m.header.msgh_id = message_id;
    nxb_mach_pack_release(m.bytes, frame, disposition);
    mach_msg_return_t e = mach_msg(&m.header, MACH_SEND_MSG | MACH_SEND_TIMEOUT, m.header.msgh_size, 0, MACH_PORT_NULL, 1000, MACH_PORT_NULL);
    if (e != MACH_MSG_SUCCESS) fprintf(stderr, "[nxb-worker] release send failed=%d\n", e);
    mach_port_deallocate(mach_task_self(), port);
}
static void release_frame(mach_port_t port, const nxb_mach_frame *frame, uint32_t disposition) { reply_frame(port, frame, NXB_MACH_RELEASE_ID, disposition); }
static void pump(void);
static void schedule_pump_locked(void) {
    if (!pump_scheduled) { pump_scheduled = 1; dispatch_async(pump_queue, ^{ pump(); }); }
}
static int append_annex_b(CMSampleBufferRef sample, const nxb_mach_frame *frame) {
    CMBlockBufferRef b = CMSampleBufferGetDataBuffer(sample);
    size_t size = b ? CMBlockBufferGetDataLength(b) : 0;
    if (!size || size > 16u * 1024u * 1024u) return 0;
    uint8_t *bytes = malloc(size); if (!bytes) return 0;
    CMFormatDescriptionRef fmt = CMSampleBufferGetFormatDescription(sample);
    const uint8_t *sps = NULL, *pps = NULL; size_t sl = 0, pl = 0, count = 0; int nal_len = 0;
    int ok = CMBlockBufferCopyDataBytes(b, 0, size, bytes) == noErr && fmt &&
        CMVideoFormatDescriptionGetH264ParameterSetAtIndex(fmt, 0, &sps, &sl, &count, &nal_len) == noErr &&
        CMVideoFormatDescriptionGetH264ParameterSetAtIndex(fmt, 1, &pps, &pl, NULL, NULL) == noErr && nal_len == 4;
    const uint8_t start[4] = {0, 0, 0, 1};
    CFArrayRef attachments = CMSampleBufferGetSampleAttachmentsArray(sample, false);
    CFDictionaryRef attachment = attachments && CFArrayGetCount(attachments) ? CFArrayGetValueAtIndex(attachments, 0) : NULL;
    int keyframe = !attachment || !CFDictionaryContainsKey(attachment, kCMSampleAttachmentKey_NotSync);
    int parameters = !bridge_video || keyframe;
    if (ok) {
        flockfile(output);
        if (bridge_video || written + size + sl + pl + 8 <= 64u * 1024u * 1024u) {
            if (bridge_video) {
                uint8_t header[12]; nxb_wire_u64(header, frame->pts_ns); nxb_wire_u32(header + 8, (uint32_t)(6 + (parameters ? 8 + sl + pl : 0) + size));
                const uint8_t aud[] = {0, 0, 0, 1, 9, 0xf0};
                ok = fwrite(header, 1, sizeof header, output) == sizeof header && fwrite(aud, 1, sizeof aud, output) == sizeof aud;
            }
            if (parameters) ok = ok && fwrite(start, 1, 4, output) == 4 && fwrite(sps, 1, sl, output) == sl && fwrite(start, 1, 4, output) == 4 && fwrite(pps, 1, pl, output) == pl;
            size_t offset = 0;
            while (ok && offset + 4 <= size) {
                uint8_t *p = bytes + offset;
                uint32_t n = ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) | ((uint32_t)p[2] << 8) | p[3];
                offset += 4;
                if (!n || n > size - offset || fwrite(start, 1, 4, output) != 4 || fwrite(bytes + offset, 1, n, output) != n) { ok = 0; break; }
                offset += n;
            }
            if (offset != size) ok = 0;
            if (ok) {
                written += size + sl + pl + 8;
                if (event_trace) {
                    flockfile(event_trace);
                    if (trace_events++ < 100000) fprintf(event_trace, "video,%llu,%llu,0,0,0\n", (unsigned long long)frame->sequence, (unsigned long long)frame->pts_ns);
                    funlockfile(event_trace);
                }
            }
        }
        funlockfile(output);
    }
    free(bytes); return ok;
}
static void encoded_frame(void *opaque, void *refcon, OSStatus status, VTEncodeInfoFlags flags, CMSampleBufferRef sample) {
    (void)opaque; (void)flags;
    job *j = refcon;
    atomic_max_ns(&encode_max_ns, monotonic_ns() - j->encode_started_ns);
    if (stall_ms) usleep(stall_ms * 1000u);
    uint64_t pipe_started = monotonic_ns();
    int ok = status == noErr && sample && CMSampleBufferDataIsReady(sample) && append_annex_b(sample, &j->frame);
    atomic_max_ns(&pipe_max_ns, monotonic_ns() - pipe_started);
    CFRelease(j->pixel);
    pthread_mutex_lock(&lock);
    if (active_sequence[j->frame.buffer_id - 1] != j->frame.sequence) failures++;
    else active_sequence[j->frame.buffer_id - 1] = 0;
    inflight--; if (ok) encoded++; else failures++;
    pthread_cond_broadcast(&drained);
    if (pending_count) schedule_pump_locked();
    pthread_mutex_unlock(&lock);
    /* The encoder callback has completed and the imported image has been
     * released before the source receives permission to write again. */
    release_frame(j->release, &j->frame, ok ? NXB_MACH_ENCODED : NXB_MACH_FAILED);
    free(j);
}
static void pump(void) {
    for (;;) {
        pthread_mutex_lock(&lock);
        if (!pending_count || inflight >= 2) { pump_scheduled = 0; pthread_cond_broadcast(&drained); pthread_mutex_unlock(&lock); return; }
        job *j = pending[0];
        for (unsigned i = 1; i < pending_count; i++) pending[i - 1] = pending[i];
        pending_count--;
        uint64_t now = monotonic_ns(), waited = now - j->queued_ns;
        atomic_max_ns(&queue_max_ns, waited);
        if (waited > NXB_RAW_QUEUE_MAX_NS) {
            active_sequence[j->frame.buffer_id - 1] = 0;
            pthread_mutex_unlock(&lock);
            /* No VT submission occurred: dropping cannot invalidate a P-frame. */
            CFRelease(j->pixel);
            release_frame(j->release, &j->frame, NXB_MACH_DROPPED);
            free(j); dropped++; raw_expired++;
            continue;
        }
        inflight++;
        j->encode_started_ns = now;
        pthread_mutex_unlock(&lock);
        CMTime pts = CMTimeMake((int64_t)j->frame.pts_ns, 1000000000), dur = CMTimeMake((int64_t)j->frame.duration_ns, 1000000000);
        OSStatus e = VTCompressionSessionEncodeFrame(encoder, j->pixel, pts, dur, NULL, j, NULL);
        if (e != noErr) encoded_frame(NULL, j, e, 0, NULL);
    }
}
static int init_encoder(void) {
    if (VTCompressionSessionCreate(NULL, 1280, 720, kCMVideoCodecType_H264, NULL, NULL, NULL, encoded_frame, NULL, &encoder) != noErr) return 0;
    VTSessionSetProperty(encoder, kVTCompressionPropertyKey_RealTime, kCFBooleanTrue);
    VTSessionSetProperty(encoder, kVTCompressionPropertyKey_AllowFrameReordering, kCFBooleanFalse);
    int bitrate = 5000000; CFNumberRef rate = CFNumberCreate(NULL, kCFNumberIntType, &bitrate);
    VTSessionSetProperty(encoder, kVTCompressionPropertyKey_AverageBitRate, rate); CFRelease(rate);
    int interval = 30; CFNumberRef n = CFNumberCreate(NULL, kCFNumberIntType, &interval);
    VTSessionSetProperty(encoder, kVTCompressionPropertyKey_MaxKeyFrameInterval, n); CFRelease(n);
    return VTCompressionSessionPrepareToEncodeFrames(encoder) == noErr;
}
static void register_surface(nxb_mach_frame_message *msg, const uint8_t id[16], uint64_t gen) {
    if (!(msg->header.msgh_bits & MACH_MSGH_BITS_COMPLEX) || msg->header.msgh_size != offsetof(nxb_mach_frame_message, trailer) ||
        msg->body.msgh_descriptor_count != 2 || msg->surface.type != MACH_MSG_PORT_DESCRIPTOR || msg->release.type != MACH_MSG_PORT_DESCRIPTOR) {
        mach_msg_destroy(&msg->header); failures++; return;
    }
    nxb_mach_frame wire; nxb_mach_unpack_frame(&wire, msg->bytes);
    mach_port_t surface_port = msg->surface.name, reply_port = msg->release.name;
    IOSurfaceRef surface = IOSurfaceLookupFromMachPort(surface_port);
    mach_port_deallocate(mach_task_self(), surface_port);
    int valid = !memcmp(wire.session_id, id, 16) && wire.generation == gen && wire.buffer_id >= 1 && wire.buffer_id <= 6 &&
        !wire.sequence && !wire.pts_ns && !wire.duration_ns && wire.width == 1280 && wire.height == 720 && surface &&
        IOSurfaceGetWidth(surface) == wire.width && IOSurfaceGetHeight(surface) == wire.height && IOSurfaceGetBytesPerElement(surface) == 4 &&
        IOSurfaceGetPixelFormat(surface) == kCVPixelFormatType_32BGRA;
    nxb_media_epoch_v1 epoch = {0}; memcpy(epoch.session_id, id, 16); epoch.generation = gen;
    nxb_video_buffer_v1 desc; nxb_video_frame_v1 unused;
    nxb_mach_frame geometry = wire; geometry.sequence = geometry.pts_ns = geometry.duration_ns = 1;
    if (valid) valid = nxb_mach_video_descriptors(&geometry, &epoch, IOSurfaceGetAllocSize(surface), IOSurfaceGetBytesPerRow(surface), NXB_COLOR_BT709, &desc, &unused);
    if (valid) {
        unsigned index = (unsigned)wire.buffer_id - 1;
        pthread_mutex_lock(&lock);
        if (registered[index] && (IOSurfaceGetID(registered[index]) != IOSurfaceGetID(surface) ||
            registered_buffers[index].allocation_bytes != desc.allocation_bytes || registered_buffers[index].planes[0].stride_bytes != desc.planes[0].stride_bytes)) valid = 0;
        for (unsigned i = 0; i < 6; i++) if (i != index && registered[i] && IOSurfaceGetID(registered[i]) == IOSurfaceGetID(surface)) valid = 0;
        if (valid && !registered[index]) { registered[index] = (IOSurfaceRef)CFRetain(surface); registered_buffers[index] = desc; registered_count++; }
        pthread_mutex_unlock(&lock);
    }
    if (surface) CFRelease(surface);
    if (valid && stale_registration_ack) {
        wire.generation++;
        wire.session_id[0] ^= 1;
    }
    reply_frame(reply_port, &wire, valid ? NXB_MACH_REGISTER_ACK_ID : NXB_MACH_REGISTER_NACK_ID, 0);
    if (!valid) failures++;
}
static void consume(nxb_mach_frame_message *msg, const uint8_t expected_id[16], uint64_t expected_gen, uint64_t *last_seq) {
    if (!(msg->header.msgh_bits & MACH_MSGH_BITS_COMPLEX) || msg->header.msgh_size != offsetof(nxb_mach_frame_message, trailer) ||
        msg->header.msgh_id != NXB_MACH_FRAME_ID || msg->body.msgh_descriptor_count != 2 ||
        msg->surface.type != MACH_MSG_PORT_DESCRIPTOR || msg->release.type != MACH_MSG_PORT_DESCRIPTOR) {
        mach_msg_destroy(&msg->header); failures++; return;
    }
    nxb_mach_frame frame; nxb_mach_unpack_frame(&frame, msg->bytes);
    mach_port_t surface_port = msg->surface.name, release_port = msg->release.name;
    /* A duplicate of a lease we still own must not receive a failure release:
     * the producer could interpret it as permission to overwrite the image
     * while the original is pending or held by VideoToolbox. */
    int duplicate_active = 0;
    if (!memcmp(frame.session_id, expected_id, 16) && frame.generation == expected_gen &&
        frame.sequence && frame.buffer_id >= 1 && frame.buffer_id <= 6) {
        pthread_mutex_lock(&lock);
        duplicate_active = active_sequence[frame.buffer_id - 1] == frame.sequence;
        pthread_mutex_unlock(&lock);
    }
    if (duplicate_active) {
        mach_port_deallocate(mach_task_self(), surface_port);
        mach_port_deallocate(mach_task_self(), release_port);
        failures++;
        return;
    }
    if (memcmp(frame.session_id, expected_id, 16) || frame.generation != expected_gen || !frame.sequence || frame.sequence <= *last_seq ||
        !frame.buffer_id || frame.buffer_id > 6 || frame.width != 1280 || frame.height != 720 || !frame.pts_ns || !frame.duration_ns) {
        mach_port_deallocate(mach_task_self(), surface_port); release_frame(release_port, &frame, NXB_MACH_FAILED); failures++; return;
    }
    *last_seq = frame.sequence; received++;
    /* Source-ready timestamps, not simulation FPS. Receiver gaps separately
     * expose scheduling delays; neither counter proves a game render rate. */
    uint64_t receive_ns = monotonic_ns();
    if (source_last_pts && frame.pts_ns > source_last_pts) {
        uint64_t gap = frame.pts_ns - source_last_pts;
        if (gap > source_gap_max_ns) source_gap_max_ns = gap;
        if (gap > 100000000ull) source_gaps_over_100ms++;
    }
    if (source_last_receive_ns && receive_ns - source_last_receive_ns > source_receive_gap_max_ns)
        source_receive_gap_max_ns = receive_ns - source_last_receive_ns;
    source_last_pts = frame.pts_ns; source_last_receive_ns = receive_ns;
    if (bridge_video) {
        const uint64_t interval = 1000000000ull / 30;
        if (bridge_next_video_pts && frame.pts_ns + 1000000ull < bridge_next_video_pts) {
            mach_port_deallocate(mach_task_self(), surface_port); release_frame(release_port, &frame, NXB_MACH_DROPPED); dropped++; return;
        }
        if (!bridge_next_video_pts || frame.pts_ns > bridge_next_video_pts + interval) bridge_next_video_pts = frame.pts_ns;
        bridge_next_video_pts += interval;
    }
    IOSurfaceRef surface = IOSurfaceLookupFromMachPort(surface_port);
    mach_port_deallocate(mach_task_self(), surface_port);
    if (!surface || IOSurfaceGetWidth(surface) != frame.width || IOSurfaceGetHeight(surface) != frame.height || IOSurfaceGetBytesPerElement(surface) != 4 || IOSurfaceGetPixelFormat(surface) != kCVPixelFormatType_32BGRA ||
        IOSurfaceGetBytesPerRow(surface) < frame.width * 4u || IOSurfaceGetBytesPerRow(surface) > IOSurfaceGetAllocSize(surface) / frame.height) {
        if (surface) CFRelease(surface); release_frame(release_port, &frame, NXB_MACH_FAILED); failures++; return;
    }
    nxb_media_epoch_v1 epoch = {0}; memcpy(epoch.session_id, expected_id, 16); epoch.generation = expected_gen;
    nxb_video_buffer_v1 descriptor; nxb_video_frame_v1 ready;
    /* BT.709 is a test assumption; the actual game color profile still needs
     * independent validation before this can be advertised as production. */
    if (!nxb_mach_video_descriptors(&frame, &epoch, IOSurfaceGetAllocSize(surface), IOSurfaceGetBytesPerRow(surface), NXB_COLOR_BT709, &descriptor, &ready)) {
        CFRelease(surface); release_frame(release_port, &frame, NXB_MACH_FAILED); failures++; return;
    }
    unsigned index = (unsigned)frame.buffer_id - 1;
    pthread_mutex_lock(&lock);
    int invalid = active_sequence[index] != 0 || !registered[index];
    if (registered[index] && IOSurfaceGetID(registered[index]) != IOSurfaceGetID(surface)) invalid = 1;
    if (registered[index] && (registered_buffers[index].allocation_bytes != descriptor.allocation_bytes ||
        registered_buffers[index].planes[0].stride_bytes != descriptor.planes[0].stride_bytes)) invalid = 1;
    for (unsigned i = 0; i < 6; i++) if (i != index && registered[i] && IOSurfaceGetID(registered[i]) == IOSurfaceGetID(surface)) invalid = 1;
    pthread_mutex_unlock(&lock);
    if (invalid) { CFRelease(surface); release_frame(release_port, &frame, NXB_MACH_FAILED); failures++; return; }
    CVPixelBufferRef pixel = NULL;
    CVReturn r = CVPixelBufferCreateWithIOSurface(NULL, surface, NULL, &pixel);
    CFRelease(surface);
    if (r != kCVReturnSuccess || !pixel) { release_frame(release_port, &frame, NXB_MACH_FAILED); failures++; return; }
    job *j = malloc(sizeof *j);
    if (!j) { CFRelease(pixel); failures++; release_frame(release_port, &frame, NXB_MACH_FAILED); return; }
    *j = (job){ .frame = frame, .release = release_port, .pixel = pixel, .queued_ns = monotonic_ns() };
    job *evicted = NULL;
    pthread_mutex_lock(&lock);
    if (pending_count == 1) {
        evicted = pending[0];
        for (unsigned i = 1; i < pending_count; i++) pending[i - 1] = pending[i];
        pending_count--; active_sequence[evicted->frame.buffer_id - 1] = 0;
    }
    active_sequence[index] = frame.sequence; pending[pending_count++] = j;
    schedule_pump_locked();
    pthread_mutex_unlock(&lock);
    if (evicted) {
        raw_replaced++;
        uint64_t n = atomic_fetch_add(&dropped, 1);
        if (n < 3) fprintf(stderr, "[nxb-worker] pending full: dropped oldest sequence=%llu for sequence=%llu\n", (unsigned long long)evicted->frame.sequence, (unsigned long long)frame.sequence);
        CFRelease(evicted->pixel); release_frame(evicted->release, &evicted->frame, NXB_MACH_DROPPED); free(evicted);
    }
}
static void consume_audio(nxb_mach_audio_message *msg, const uint8_t id[16], uint64_t gen) {
    if (msg->header.msgh_bits & MACH_MSGH_BITS_COMPLEX) { mach_msg_destroy(&msg->header); failures++; return; }
    if (msg->header.msgh_size < sizeof(mach_msg_header_t) + 56) { failures++; return; }
    const uint8_t *p = msg->bytes;
    uint64_t wire_generation = nxb_wire_get64(p+16), seq = nxb_wire_get64(p+24), first = nxb_wire_get64(p+32), pts = nxb_wire_get64(p+40);
    uint32_t frames = nxb_wire_get32(p+48), flags = nxb_wire_get32(p+52);
    if (memcmp(p, id, 16) || wire_generation != gen || !seq || !pts || !frames || frames > NXB_MACH_AUDIO_MAX || flags > 1 ||
        first > UINT64_MAX - frames || msg->header.msgh_size != sizeof(mach_msg_header_t) + 56 + frames * 8 ||
        (audio_last_seq && (seq <= audio_last_seq || pts <= audio_last_pts || first < audio_last_sample ||
          ((seq != audio_last_seq + 1 || first != audio_last_sample) && !(flags & 1))))) { failures++; return; }
    nxb_media_epoch_v1 epoch = {0}; memcpy(epoch.session_id, id, 16); epoch.generation = gen;
    nxb_audio_buffer_v1 buffer; nxb_audio_chunk_v1 chunk;
    if (!nxb_mach_audio_descriptors(p, &epoch, 1, sizeof(audio_buffer), &buffer, &chunk)) { failures++; return; }
    if (!audio_registration.buffer_id) audio_registration = buffer;
    if (audio_busy_sequence || audio_registration.buffer_id != buffer.buffer_id ||
        audio_registration.allocation_bytes != buffer.allocation_bytes ||
        memcmp(&audio_registration.epoch, &buffer.epoch, sizeof buffer.epoch)) { failures++; return; }
    if (!audio_last_seq || (flags & NXB_AUDIO_DISCONTINUITY)) { audio_anchor_sample = first; audio_anchor_pts = pts; }
    else {
        uint64_t delta = first - audio_anchor_sample, seconds = delta / NXB_AUDIO_RATE;
        uint64_t fraction = (delta % NXB_AUDIO_RATE) * 1000000000ull / NXB_AUDIO_RATE;
        if (seconds > (UINT64_MAX - fraction) / 1000000000ull || audio_anchor_pts > UINT64_MAX - (seconds * 1000000000ull + fraction)) { failures++; return; }
        uint64_t expected = audio_anchor_pts + seconds * 1000000000ull + fraction;
        if (pts != expected) { failures++; return; }
    }
    audio_busy_sequence = chunk.sequence;
    for (unsigned i = 0; i < frames * 2; i++) {
        uint32_t bits = nxb_wire_get32(p + 56 + i*4); memcpy(&audio_buffer[i], &bits, 4);
        if (!isfinite(audio_buffer[i])) { audio_busy_sequence = 0; failures++; return; }
        if (audio_buffer[i] != 0) audio_nonzero++;
    }
    if (audio_output && audio_frames < 48000u * 60u && fwrite(audio_buffer, sizeof(float), frames * 2, audio_output) != frames * 2) failures++;
    if (bridge_audio_fd >= 0) {
        uint8_t packet[16 + NXB_MACH_AUDIO_MAX * 2 * 4];
        nxb_wire_u64(packet, pts); nxb_wire_u32(packet + 8, frames);
        nxb_wire_u32(packet + 12, flags | (bridge_audio_discontinuity ? NXB_AUDIO_DISCONTINUITY : 0));
        memcpy(packet + 16, audio_buffer, (size_t)frames * 2 * 4);
        size_t bytes = 16 + (size_t)frames * 2 * 4;
        ssize_t sent = send(bridge_audio_fd, packet, bytes, MSG_DONTWAIT);
        if (sent != (ssize_t)bytes) {
            bridge_audio_dropped++;
            if (sent < 0 && (errno == EAGAIN || errno == EWOULDBLOCK || errno == ENOBUFS)) {
                bridge_audio_full++;
            } else {
                bridge_audio_other++;
                if (bridge_audio_other <= 3) fprintf(stderr, "[nxb-worker] audio bridge send error=%d\n", sent < 0 ? errno : 0);
            }
            bridge_audio_discontinuity = 1;
        } else {
            bridge_audio_discontinuity = 0;
        }
    }
    if (event_trace) {
        flockfile(event_trace);
        if (trace_events++ < 100000) fprintf(event_trace, "audio,%llu,%llu,%llu,%u,%u\n", (unsigned long long)seq, (unsigned long long)pts, (unsigned long long)first, frames, flags);
        funlockfile(event_trace);
    }
    nxb_audio_release_v1 release = { .struct_size = sizeof(release), .abi_version = NXB_MEDIA_ABI_V1,
        .epoch = epoch, .sequence = chunk.sequence, .buffer_id = chunk.buffer_id, .disposition = NXB_RELEASE_DROPPED };
    if (release.sequence != audio_busy_sequence || release.buffer_id != audio_registration.buffer_id ||
        memcmp(&release.epoch, &audio_registration.epoch, sizeof epoch)) { failures++; return; }
    audio_busy_sequence = 0; audio_releases++;
    audio_last_seq = seq; audio_last_sample = first + frames; audio_last_pts = pts;
    audio_blocks++; audio_frames += frames; if (flags & 1) audio_discontinuities++;
}
int main(int argc, char **argv) {
    @autoreleasepool {
        if (argc < 2) { fprintf(stderr, "usage: nxnative_mach_worker COMMAND [ARGS...]\n"); return 2; }
        const char *path = getenv("NXB_WORKER_OUTPUT"), *sid = getenv("NXB_SESSION_ID"), *g = getenv("NXB_VIDEO_GENERATION");
        const char *video_fd = getenv("NXB_BRIDGE_VIDEO_FD"), *audio_fd = getenv("NXB_BRIDGE_AUDIO_FD");
        if ((!path && !video_fd) || !sid || strlen(sid) != 32 || !g) return 2;
        const char *stall = getenv("NXB_WORKER_STALL_MS");
        if (stall) { char *end = NULL; long n = strtol(stall, &end, 10); if (!*stall || *end || n < 0 || n > 5000) return 2; stall_ms = (unsigned)n; }
        stale_registration_ack = getenv("NXB_WORKER_STALE_ACK") != NULL;
        uint8_t id[16];
        for (int i = 0; i < 16; i++) { char p[3] = {sid[2*i], sid[2*i+1], 0}, *end = NULL; unsigned long v = strtoul(p, &end, 16); if (end != p+2 || *end) return 2; id[i] = (uint8_t)v; }
        char *end = NULL; uint64_t gen = strtoull(g, &end, 10); if (!gen || *end) return 2;
        if (video_fd) {
            char *end = NULL; long fd = strtol(video_fd, &end, 10); if (!*video_fd || *end || fd < 3 || fd > 1024) return 2;
            int copied = dup((int)fd); if (copied < 0) return 2;
            if (fcntl(copied, F_SETFD, FD_CLOEXEC) < 0) { close(copied); return 2; }
            int yes = 1; setsockopt(copied, SOL_SOCKET, SO_NOSIGPIPE, &yes, sizeof yes);
            output = fdopen(copied, "wb"); bridge_video = 1;
            close((int)fd);
            if (output) setvbuf(output, NULL, _IONBF, 0);
        } else output = fopen(path, "wx");
        if (!output || !init_encoder()) return 2;
        if (audio_fd) {
            char *end = NULL; long fd = strtol(audio_fd, &end, 10); if (!*audio_fd || *end || fd < 3 || fd > 1024) return 2;
            bridge_audio_fd = dup((int)fd); if (bridge_audio_fd < 0) return 2;
            if (fcntl(bridge_audio_fd, F_SETFD, FD_CLOEXEC) < 0) { close(bridge_audio_fd); return 2; }
            close((int)fd);
        }
        pump_queue = dispatch_queue_create("org.nxbridge.mach-worker-pump", DISPATCH_QUEUE_SERIAL);
        if (!pump_queue) return 2;
        const char *audio_path = getenv("NXB_WORKER_AUDIO");
        if (audio_path && *audio_path && !(audio_output = fopen(audio_path, "wx"))) return 2;
        const char *trace_path = getenv("NXB_WORKER_TRACE");
        if (trace_path && *trace_path) {
            event_trace = fopen(trace_path, "wx"); if (!event_trace) return 2;
            fputs("kind,sequence,pts_ns,first_sample,frames,flags\n", event_trace);
        }
        mach_port_t receive = MACH_PORT_NULL;
        assert(mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &receive) == KERN_SUCCESS);
        mach_port_limits_t limits = { .mpl_qlimit = 3 };
        assert(mach_port_set_attributes(mach_task_self(), receive, MACH_PORT_LIMITS_INFO, (mach_port_info_t)&limits, MACH_PORT_LIMITS_INFO_COUNT) == KERN_SUCCESS);
        assert(mach_port_insert_right(mach_task_self(), receive, receive, MACH_MSG_TYPE_MAKE_SEND) == KERN_SUCCESS);
        mach_port_array_t old = NULL; mach_msg_type_number_t count = 0;
        assert(mach_ports_lookup(mach_task_self(), &old, &count) == KERN_SUCCESS && count <= 3);
        mach_port_t ports[3] = {MACH_PORT_NULL, MACH_PORT_NULL, MACH_PORT_NULL};
        for (unsigned i = 0; i < count; i++) ports[i] = old[i];
        assert(!MACH_PORT_VALID(ports[1])); ports[1] = receive;
        assert(mach_ports_register(mach_task_self(), ports, 3) == KERN_SUCCESS);
        setenv("NXB_MACH_MEDIA", "1", 1);
        signal(SIGTERM, stop_signal); signal(SIGINT, stop_signal);
        pid_t child = -1; int result = posix_spawn(&child, argv[1], NULL, NULL, argv + 1, environ);
        assert(mach_ports_register(mach_task_self(), old, count) == KERN_SUCCESS && result == 0);
        for (mach_msg_type_number_t i = 0; i < count; i++) if (MACH_PORT_VALID(old[i])) mach_port_deallocate(mach_task_self(), old[i]);
        vm_deallocate(mach_task_self(), (vm_address_t)old, count * sizeof(*old));
        const char *seconds = getenv("NXB_WORKER_TEST_SECONDS"); long max_seconds = seconds ? strtol(seconds, NULL, 10) : 0;
        struct timespec start; clock_gettime(CLOCK_MONOTONIC, &start);
        int sent_term = 0, child_done = 0, status = 0; uint64_t last_seq = 0;
        uint64_t report_at = monotonic_ns();
        for (;;) {
            union { nxb_mach_frame_message video; nxb_mach_audio_message audio; } msg = {0};
            mach_msg_return_t r = mach_msg(&msg.video.header, MACH_RCV_MSG | MACH_RCV_TIMEOUT, 0, sizeof msg, receive, 100, MACH_PORT_NULL);
            if (r == MACH_MSG_SUCCESS && msg.video.header.msgh_id == NXB_MACH_AUDIO_ID) consume_audio(&msg.audio, id, gen);
            else if (r == MACH_MSG_SUCCESS && msg.video.header.msgh_id == NXB_MACH_REGISTER_ID) register_surface(&msg.video, id, gen);
            else if (r == MACH_MSG_SUCCESS) consume(&msg.video, id, gen, &last_seq);
            else if (r != MACH_RCV_TIMED_OUT) { fprintf(stderr, "[nxb-worker] receive error=%d\n", r); break; }
            if (!child_done && waitpid(child, &status, WNOHANG) == child) child_done = 1;
            if (child_done && r == MACH_RCV_TIMED_OUT) break;
            struct timespec now; clock_gettime(CLOCK_MONOTONIC, &now);
            if (monotonic_ns() - report_at >= 10000000000ull) { report_video_timing(); report_at = monotonic_ns(); }
            if (!child_done && !sent_term && (stop_requested || (max_seconds > 0 && now.tv_sec - start.tv_sec >= max_seconds))) { kill(child, SIGTERM); sent_term = 1; }
        }
        for (;;) {
            dispatch_sync(pump_queue, ^{});
            VTCompressionSessionCompleteFrames(encoder, kCMTimeInvalid);
            pthread_mutex_lock(&lock);
            if (!pending_count && !inflight && !pump_scheduled) { pthread_mutex_unlock(&lock); break; }
            struct timespec deadline; clock_gettime(CLOCK_REALTIME, &deadline); deadline.tv_sec++;
            int timed = pthread_cond_timedwait(&drained, &lock, &deadline);
            pthread_mutex_unlock(&lock);
            if (timed) { fprintf(stderr, "[nxb-worker] drain timed out; buffers remain isolated\n"); failures++; break; }
        }
        pthread_mutex_lock(&lock); fprintf(stderr, "[nxb-worker] generation=%llu registered=%u received=%llu encoded=%llu dropped=%llu failed=%llu inflight=%u child_status=%d\n", (unsigned long long)gen, registered_count, (unsigned long long)received, (unsigned long long)encoded, (unsigned long long)dropped, (unsigned long long)failures, inflight, status); unsigned outstanding = inflight; pthread_mutex_unlock(&lock);
        fprintf(stderr, "[nxb-worker] audio blocks=%llu frames=%llu nonzero=%llu discontinuities=%llu releases=%llu busy=%llu bridge_dropped=%llu bridge_full=%llu bridge_other=%llu\n", (unsigned long long)audio_blocks, (unsigned long long)audio_frames, (unsigned long long)audio_nonzero, (unsigned long long)audio_discontinuities, (unsigned long long)audio_releases, (unsigned long long)audio_busy_sequence, (unsigned long long)bridge_audio_dropped, (unsigned long long)bridge_audio_full, (unsigned long long)bridge_audio_other);
        report_video_timing();
        VTCompressionSessionInvalidate(encoder); CFRelease(encoder);
        for (unsigned i = 0; i < 6; i++) if (registered[i]) CFRelease(registered[i]);
        fclose(output); if (audio_output) fclose(audio_output); if (event_trace) fclose(event_trace); if (bridge_audio_fd >= 0) close(bridge_audio_fd);
        mach_port_deallocate(mach_task_self(), receive); mach_port_mod_refs(mach_task_self(), receive, MACH_PORT_RIGHT_RECEIVE, -1);
        return child_done && !outstanding && received && encoded && !failures && WIFEXITED(status) && WEXITSTATUS(status) == 0 ? 0 : 1;
    }
}
