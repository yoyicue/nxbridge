/* Exercise the real worker PCM receive and Unix datagram bridge without a game. */
#define main nxb_worker_program_main
#include "nxnative_mach_worker.m"
#undef main
#include <errno.h>

static nxb_mach_audio_message audio_message(uint64_t seq, uint64_t first, uint64_t pts) {
    nxb_mach_audio_message msg = {0};
    msg.header.msgh_size = sizeof(mach_msg_header_t) + 56 + 240 * 8;
    msg.bytes[0] = 1;
    nxb_wire_u64(msg.bytes + 16, 2);
    nxb_wire_u64(msg.bytes + 24, seq);
    nxb_wire_u64(msg.bytes + 32, first);
    nxb_wire_u64(msg.bytes + 40, pts);
    nxb_wire_u32(msg.bytes + 48, 240);
    return msg;
}

int main(void) {
    const uint8_t id[16] = {1};
    int sockets[2];
    assert(socketpair(AF_UNIX, SOCK_DGRAM, 0, sockets) == 0);
    bridge_audio_fd = sockets[0];
    assert(fcntl(sockets[1], F_SETFL, O_NONBLOCK) == 0);
    uint8_t filler[1936] = {0};
    unsigned count = 0;
    while (send(sockets[0], filler, sizeof filler, MSG_DONTWAIT) == (ssize_t)sizeof filler)
        assert(++count < 100000);
    assert(errno == EAGAIN || errno == EWOULDBLOCK || errno == ENOBUFS);
    nxb_mach_audio_message first = audio_message(1, 0, 1000000000ull);
    consume_audio(&first, id, 2);
    assert(audio_blocks == 1 && bridge_audio_dropped == 1 && bridge_audio_full == 1 && bridge_audio_other == 0);
    while (recv(sockets[1], filler, sizeof filler, 0) > 0) {}
    assert(errno == EAGAIN || errno == EWOULDBLOCK);

    nxb_mach_audio_message second = audio_message(2, 240, 1005000000ull);
    consume_audio(&second, id, 2);
    assert(recv(sockets[1], filler, sizeof filler, 0) == (ssize_t)sizeof filler);
    assert(nxb_wire_get32(filler + 12) & NXB_AUDIO_DISCONTINUITY);
    nxb_mach_audio_message third = audio_message(3, 480, 1010000000ull);
    consume_audio(&third, id, 2);
    assert(recv(sockets[1], filler, sizeof filler, 0) == (ssize_t)sizeof filler);
    assert(nxb_wire_get32(filler + 12) == 0);
    assert(audio_blocks == 3 && failures == 0);
    close(sockets[0]); close(sockets[1]);
    return 0;
}
