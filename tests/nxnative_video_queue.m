/* Latest raw frame selection and expiry use actual IOSurface leases and Mach
 * reply ports. The expired job must never reach VideoToolbox. */
#define main nxb_worker_program_main
#include "nxnative_mach_worker.m"
#undef main

static mach_port_t reply_port(void) {
    mach_port_t p = MACH_PORT_NULL;
    assert(mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &p) == KERN_SUCCESS);
    assert(mach_port_insert_right(mach_task_self(), p, p, MACH_MSG_TYPE_MAKE_SEND) == KERN_SUCCESS);
    return p;
}
static nxb_mach_frame_message make_message(IOSurfaceRef s, mach_port_t reply, nxb_mach_frame f, int kind) {
    nxb_mach_frame_message msg = {0};
    msg.header.msgh_bits = MACH_MSGH_BITS_COMPLEX;
    msg.header.msgh_size = (mach_msg_size_t)offsetof(nxb_mach_frame_message, trailer);
    msg.header.msgh_id = kind; msg.body.msgh_descriptor_count = 2;
    msg.surface.type = msg.release.type = MACH_MSG_PORT_DESCRIPTOR;
    msg.surface.name = IOSurfaceCreateMachPort(s); msg.release.name = reply;
    assert(MACH_PORT_VALID(msg.surface.name));
    nxb_mach_pack_frame(msg.bytes, &f); return msg;
}
static void expect_release(mach_port_t port, nxb_mach_frame f, int kind, uint32_t disposition) {
    nxb_mach_release_message m = {0};
    assert(mach_msg(&m.header, MACH_RCV_MSG | MACH_RCV_TIMEOUT, 0, sizeof m, port, 100, MACH_PORT_NULL) == MACH_MSG_SUCCESS);
    assert(m.header.msgh_id == kind);
    assert(!memcmp(m.bytes, f.session_id, 16));
    assert(nxb_wire_get64(m.bytes + 16) == f.generation);
    assert(nxb_wire_get64(m.bytes + 24) == f.sequence);
    assert(nxb_wire_get64(m.bytes + 32) == f.buffer_id);
    assert(nxb_wire_get32(m.bytes + 40) == disposition);
    assert(mach_msg(&m.header, MACH_RCV_MSG | MACH_RCV_TIMEOUT, 0, sizeof m, port, 1, MACH_PORT_NULL) == MACH_RCV_TIMED_OUT);
    assert(mach_port_mod_refs(mach_task_self(), port, MACH_PORT_RIGHT_RECEIVE, -1) == KERN_SUCCESS);
}
int main(void) {
    @autoreleasepool {
        const uint8_t id[16] = {1};
        IOSurfaceRef surfaces[3]; mach_port_t replies[3]; nxb_mach_frame frames[3];
        uint64_t last = 0;
        pump_scheduled = 1; /* Deterministically hold the consumer. */
        for (unsigned i = 0; i < 3; i++) {
            NSDictionary *props = @{(__bridge NSString *)kIOSurfaceWidth: @1280,
                (__bridge NSString *)kIOSurfaceHeight: @720,
                (__bridge NSString *)kIOSurfaceBytesPerElement: @4,
                (__bridge NSString *)kIOSurfacePixelFormat: @(kCVPixelFormatType_32BGRA)};
            surfaces[i] = IOSurfaceCreate((__bridge CFDictionaryRef)props); assert(surfaces[i]);
            nxb_mach_frame f = { .session_id = {1}, .generation = 7, .buffer_id = i + 1, .width = 1280, .height = 720 };
            mach_port_t reg = reply_port();
            nxb_mach_frame_message msg = make_message(surfaces[i], reg, f, NXB_MACH_REGISTER_ID);
            register_surface(&msg, id, 7);
            expect_release(reg, f, NXB_MACH_REGISTER_ACK_ID, 0);
            f.sequence = i + 1; f.pts_ns = (i + 1) * 33333333ull; f.duration_ns = 33333333;
            frames[i] = f; replies[i] = reply_port();
            msg = make_message(surfaces[i], replies[i], f, NXB_MACH_FRAME_ID);
            consume(&msg, id, 7, &last);
            assert(pending_count == 1 && pending[0]->frame.sequence == i + 1);
            assert(active_sequence[i] == i + 1 && inflight == 0);
            if (i) {
                assert(active_sequence[i - 1] == 0);
                expect_release(replies[i - 1], frames[i - 1], NXB_MACH_RELEASE_ID, NXB_MACH_DROPPED);
            }
        }
        assert(atomic_load(&raw_replaced) == 2 && !atomic_load(&encoded));
        pending[0]->queued_ns = monotonic_ns() - NXB_RAW_QUEUE_MAX_NS - 1;
        pump(); /* encoder == NULL: an unintended submission would fail this test. */
        assert(!pending_count && !inflight && !pump_scheduled && !active_sequence[2]);
        assert(atomic_load(&raw_expired) == 1 && atomic_load(&dropped) == 3 && !atomic_load(&failures));
        expect_release(replies[2], frames[2], NXB_MACH_RELEASE_ID, NXB_MACH_DROPPED);
        for (unsigned i = 0; i < 3; i++) { CFRelease(registered[i]); CFRelease(surfaces[i]); }
        puts("PASS: raw queue latest frame, expiry, exact-once epoch/sequence lease release");
    }
    return 0;
}
