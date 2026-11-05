#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include "../adapters/nxnative/video_probe.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>

int main(int argc, char **argv) {
    assert(argc == 2);
    @autoreleasepool {
        setenv("NXB_VIDEO_PROBE", argv[1], 1);
        setenv("NXB_VIDEO_GENERATION", "42", 1);
        setenv("NXB_SESSION_ID", "00112233445566778899aabbccddeeff", 1);
        id<MTLDevice> device = MTLCreateSystemDefaultDevice();
        assert(device);
        nxb_probe_ticket t, replacement, extra;
        assert(nxb_video_probe_claim(device, 1.0 / 30, &t));
        assert(!nxb_video_probe_claim(device, 1.0 / 30, &extra));
        nxb_probe_ticket wrong = t; wrong.session_id[0] ^= 1;
        nxb_video_probe_complete(wrong, 0);
        wrong = t; wrong.generation++;
        nxb_video_probe_complete(wrong, 0);
        assert(!nxb_video_probe_claim(device, 1.0 / 30, &extra));
        nxb_video_probe_complete(t, 0);
        assert(nxb_video_probe_claim(device, 1.0 / 30, &replacement));
        nxb_video_probe_complete(t, 0); /* stale sequence must not free replacement */
        assert(!nxb_video_probe_claim(device, 1.0 / 30, &extra));
        nxb_video_probe_complete(replacement, 0);

        id<MTLCommandQueue> queue = [device newCommandQueue]; assert(queue);
        for (int i = 0; i < 30; i++) {
            nxb_probe_ticket ticket;
            id<MTLTexture> texture = nxb_video_probe_claim(device, 1.0 / 30, &ticket);
            if (!texture) continue;
            MTLRenderPassDescriptor *rp = [MTLRenderPassDescriptor renderPassDescriptor];
            rp.colorAttachments[0].texture = texture;
            rp.colorAttachments[0].loadAction = MTLLoadActionClear;
            rp.colorAttachments[0].storeAction = MTLStoreActionStore;
            rp.colorAttachments[0].clearColor = MTLClearColorMake(i / 30.0, 0.5, 0.2, 1);
            id<MTLCommandBuffer> cb = [queue commandBuffer];
            id<MTLRenderCommandEncoder> e = [cb renderCommandEncoderWithDescriptor:rp]; assert(e);
            [e endEncoding]; [cb commit]; [cb waitUntilCompleted];
            assert(cb.status == MTLCommandBufferStatusCompleted);
            nxb_video_probe_complete(ticket, 1);
        }
        nxb_video_probe_shutdown();
        struct stat st; assert(stat(argv[1], &st) == 0 && st.st_size > 0);
        puts("video probe GPU, encoder and stale lease checks passed");
    }
    return 0;
}
