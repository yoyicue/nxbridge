/* Private local Mach protocol v1. The byte payload is encoded explicitly;
 * Mach port names and native ABI structure padding are never wire values. */
#ifndef NXBRIDGE_MACH_MEDIA_H
#define NXBRIDGE_MACH_MEDIA_H
#include <mach/mach.h>
#include <stdint.h>
#include <stddef.h>
#include <string.h>

enum { NXB_MACH_FRAME_ID = 0x4e580101, NXB_MACH_RELEASE_ID = 0x4e580102, NXB_MACH_AUDIO_ID = 0x4e580103,
       NXB_MACH_REGISTER_ID = 0x4e580104, NXB_MACH_REGISTER_ACK_ID = 0x4e580105, NXB_MACH_REGISTER_NACK_ID = 0x4e580106,
       NXB_MACH_PAYLOAD = 64, NXB_MACH_AUDIO_MAX = 480 };
enum { NXB_MACH_ENCODED = 1, NXB_MACH_DROPPED = 2, NXB_MACH_FAILED = 3 };
typedef struct {
    uint8_t session_id[16];
    uint64_t generation, sequence, pts_ns, duration_ns, buffer_id;
    uint32_t width, height;
} nxb_mach_frame;
typedef struct {
    mach_msg_header_t header;
    mach_msg_body_t body;
    mach_msg_port_descriptor_t surface, release;
    uint8_t bytes[NXB_MACH_PAYLOAD];
    mach_msg_trailer_t trailer;
} nxb_mach_frame_message;
typedef struct {
    mach_msg_header_t header;
    uint8_t bytes[44]; /* session id, generation, sequence, buffer id, disposition */
    mach_msg_trailer_t trailer;
} nxb_mach_release_message;
typedef struct {
    mach_msg_header_t header;
    uint8_t bytes[56 + NXB_MACH_AUDIO_MAX * 2 * 4];
    mach_msg_trailer_t trailer;
} nxb_mach_audio_message;

static inline void nxb_wire_u64(uint8_t *p, uint64_t v) { for (unsigned i = 0; i < 8; i++) p[i] = (uint8_t)(v >> (8 * i)); }
static inline uint64_t nxb_wire_get64(const uint8_t *p) { uint64_t x = 0; for (unsigned i = 0; i < 8; i++) x |= (uint64_t)p[i] << (8 * i); return x; }
static inline void nxb_wire_u32(uint8_t *p, uint32_t v) { for (unsigned i = 0; i < 4; i++) p[i] = (uint8_t)(v >> (8 * i)); }
static inline uint32_t nxb_wire_get32(const uint8_t *p) { uint32_t x = 0; for (unsigned i = 0; i < 4; i++) x |= (uint32_t)p[i] << (8 * i); return x; }
static inline void nxb_mach_pack_frame(uint8_t *p, const nxb_mach_frame *f) {
    memcpy(p, f->session_id, 16); nxb_wire_u64(p + 16, f->generation); nxb_wire_u64(p + 24, f->sequence);
    nxb_wire_u64(p + 32, f->pts_ns); nxb_wire_u64(p + 40, f->duration_ns); nxb_wire_u64(p + 48, f->buffer_id);
    nxb_wire_u32(p + 56, f->width); nxb_wire_u32(p + 60, f->height);
}
static inline void nxb_mach_unpack_frame(nxb_mach_frame *f, const uint8_t *p) {
    memcpy(f->session_id, p, 16); f->generation = nxb_wire_get64(p + 16); f->sequence = nxb_wire_get64(p + 24);
    f->pts_ns = nxb_wire_get64(p + 32); f->duration_ns = nxb_wire_get64(p + 40); f->buffer_id = nxb_wire_get64(p + 48);
    f->width = nxb_wire_get32(p + 56); f->height = nxb_wire_get32(p + 60);
}
static inline void nxb_mach_pack_release(uint8_t *p, const nxb_mach_frame *f, uint32_t disposition) {
    memcpy(p, f->session_id, 16); nxb_wire_u64(p + 16, f->generation); nxb_wire_u64(p + 24, f->sequence); nxb_wire_u64(p + 32, f->buffer_id);
    nxb_wire_u32(p + 40, disposition);
}
#endif
