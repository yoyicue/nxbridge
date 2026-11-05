/* Exercise the actual worker receive path with a live Mach reply port.
 * No game, Metal encoder or client device is needed. */
#define main nxb_worker_program_main
#include "nxnative_mach_worker.m"
#undef main

static mach_port_t send_and_receive_port(void) {
    mach_port_t port = MACH_PORT_NULL;
    assert(mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &port) == KERN_SUCCESS);
    assert(mach_port_insert_right(mach_task_self(), port, port, MACH_MSG_TYPE_MAKE_SEND) == KERN_SUCCESS);
    return port;
}

static void check_reply(uint64_t seq, uint64_t current_seq, int should_reply) {
    const uint8_t id[16] = {1};
    nxb_mach_frame frame = { .session_id = {1}, .generation = 2, .sequence = seq,
        .pts_ns = 1, .duration_ns = 1, .buffer_id = 1, .width = 1280, .height = 720 };
    mach_port_t surface = send_and_receive_port(), reply = send_and_receive_port();
    nxb_mach_frame_message msg = {0};
    msg.header.msgh_bits = MACH_MSGH_BITS_COMPLEX;
    msg.header.msgh_size = (mach_msg_size_t)offsetof(nxb_mach_frame_message, trailer);
    msg.header.msgh_id = NXB_MACH_FRAME_ID;
    msg.body.msgh_descriptor_count = 2;
    msg.surface.type = MACH_MSG_PORT_DESCRIPTOR; msg.surface.name = surface;
    msg.release.type = MACH_MSG_PORT_DESCRIPTOR; msg.release.name = reply;
    nxb_mach_pack_frame(msg.bytes, &frame);
    active_sequence[0] = current_seq;
    uint64_t last = 42;
    consume(&msg, id, 2, &last);
    assert(active_sequence[0] == current_seq && last == 42);
    nxb_mach_release_message response = {0};
    mach_msg_return_t r = mach_msg(&response.header, MACH_RCV_MSG | MACH_RCV_TIMEOUT,
        0, sizeof response, reply, 10, MACH_PORT_NULL);
    if (should_reply) {
        assert(r == MACH_MSG_SUCCESS);
        assert(response.header.msgh_id == NXB_MACH_RELEASE_ID);
        assert(nxb_wire_get64(response.bytes + 24) == seq);
        assert(nxb_wire_get32(response.bytes + 40) == NXB_MACH_FAILED);
    } else {
        assert(r == MACH_RCV_TIMED_OUT);
    }
    assert(mach_port_mod_refs(mach_task_self(), surface, MACH_PORT_RIGHT_RECEIVE, -1) == KERN_SUCCESS);
    assert(mach_port_mod_refs(mach_task_self(), reply, MACH_PORT_RIGHT_RECEIVE, -1) == KERN_SUCCESS);
}

static nxb_mach_frame_message message(IOSurfaceRef surface, mach_port_t reply, nxb_mach_frame frame, int message_id) {
    nxb_mach_frame_message msg = {0};
    msg.header.msgh_bits = MACH_MSGH_BITS_COMPLEX;
    msg.header.msgh_size = (mach_msg_size_t)offsetof(nxb_mach_frame_message, trailer);
    msg.header.msgh_id = message_id;
    msg.body.msgh_descriptor_count = 2;
    msg.surface.type = MACH_MSG_PORT_DESCRIPTOR; msg.surface.name = IOSurfaceCreateMachPort(surface);
    msg.release.type = MACH_MSG_PORT_DESCRIPTOR; msg.release.name = reply;
    assert(MACH_PORT_VALID(msg.surface.name));
    nxb_mach_pack_frame(msg.bytes, &frame);
    return msg;
}

static void check_pending_frame(void) {
    const uint8_t id[16] = {1};
    NSDictionary *props = @{(__bridge NSString *)kIOSurfaceWidth: @1280,
                            (__bridge NSString *)kIOSurfaceHeight: @720,
                            (__bridge NSString *)kIOSurfaceBytesPerElement: @4,
                            (__bridge NSString *)kIOSurfacePixelFormat: @(kCVPixelFormatType_32BGRA)};
    IOSurfaceRef surface = IOSurfaceCreate((__bridge CFDictionaryRef)props); assert(surface);
    nxb_mach_frame frame = { .session_id = {1}, .generation = 2, .buffer_id = 1, .width = 1280, .height = 720 };
    mach_port_t reg_reply = send_and_receive_port();
    nxb_mach_frame_message reg = message(surface, reg_reply, frame, NXB_MACH_REGISTER_ID);
    register_surface(&reg, id, 2);
    nxb_mach_release_message ack = {0};
    assert(mach_msg(&ack.header, MACH_RCV_MSG | MACH_RCV_TIMEOUT, 0, sizeof ack, reg_reply, 100, MACH_PORT_NULL) == MACH_MSG_SUCCESS);
    assert(ack.header.msgh_id == NXB_MACH_REGISTER_ACK_ID);
    assert(mach_port_mod_refs(mach_task_self(), reg_reply, MACH_PORT_RIGHT_RECEIVE, -1) == KERN_SUCCESS);

    frame.sequence = 42; frame.pts_ns = 100; frame.duration_ns = 10;
    mach_port_t original_reply = send_and_receive_port(), duplicate_reply = send_and_receive_port();
    nxb_mach_frame_message original = message(surface, original_reply, frame, NXB_MACH_FRAME_ID);
    nxb_mach_frame_message duplicate = message(surface, duplicate_reply, frame, NXB_MACH_FRAME_ID);
    uint64_t last = 0;
    pump_scheduled = 1; /* Hold the real imported image in the pending queue. */
    consume(&original, id, 2, &last);
    assert(last == 42 && pending_count == 1 && active_sequence[0] == 42);
    consume(&duplicate, id, 2, &last);
    assert(last == 42 && pending_count == 1 && active_sequence[0] == 42);
    nxb_mach_release_message out = {0};
    assert(mach_msg(&out.header, MACH_RCV_MSG | MACH_RCV_TIMEOUT, 0, sizeof out, duplicate_reply, 10, MACH_PORT_NULL) == MACH_RCV_TIMED_OUT);
    assert(mach_msg(&out.header, MACH_RCV_MSG | MACH_RCV_TIMEOUT, 0, sizeof out, original_reply, 10, MACH_PORT_NULL) == MACH_RCV_TIMED_OUT);

    job *j = pending[0]; pending_count = 0; active_sequence[0] = 0; pump_scheduled = 0;
    CFRelease(j->pixel); release_frame(j->release, &j->frame, NXB_MACH_DROPPED); free(j);
    assert(mach_msg(&out.header, MACH_RCV_MSG | MACH_RCV_TIMEOUT, 0, sizeof out, original_reply, 100, MACH_PORT_NULL) == MACH_MSG_SUCCESS);
    assert(nxb_wire_get64(out.bytes + 24) == 42 && nxb_wire_get32(out.bytes + 40) == NXB_MACH_DROPPED);
    assert(mach_port_mod_refs(mach_task_self(), original_reply, MACH_PORT_RIGHT_RECEIVE, -1) == KERN_SUCCESS);
    assert(mach_port_mod_refs(mach_task_self(), duplicate_reply, MACH_PORT_RIGHT_RECEIVE, -1) == KERN_SUCCESS);
    CFRelease(registered[0]); registered[0] = NULL; registered_count = 0; CFRelease(surface);
}

int main(void) {
    @autoreleasepool {
        check_reply(42, 42, 0); /* duplicate of a buffer still owned by the worker */
        check_reply(41, 42, 1); /* a different, older lease may be rejected normally */
        active_sequence[0] = 0;
        check_pending_frame();
    }
    return 0;
}
