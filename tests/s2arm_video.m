/* SPDX-License-Identifier: MIT
 * Synthetic pixels through the real S2Arm presentation queue and NXBridge sink.
 * The upstream stubs replace unrelated guest services, not Metal or the sink.
 */
#import <Metal/Metal.h>
#import <QuartzCore/CAMetalLayer.h>
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#include "nvn_obj.h"
#include "host_stubs.h"
#include "nxbridge_video_probe.h"

void mtl_init(void);
uint64_t mtl_ext_tex(void *texture);
uint64_t mtl_present_ext(uint64_t serial, double period, void *event, uint64_t value, const int crop[4]);
void nvn_be_present_wait(uint64_t ticket);
void nvn_be_drain(void);

int main(int argc, char **argv) {
    assert(argc == 2 || (argc == 3 && !strcmp(argv[2], "--pressure")));
    int pressure = argc == 3;
    @autoreleasepool {
        assert(g_layer == nil);
        setenv("NXB_VIDEO_PROBE", argv[1], 1);
        setenv("NXB_SESSION_ID", "00112233445566778899aabbccddeeff", 1);
        setenv("NXB_VIDEO_GENERATION", "127", 1);
        setenv("DM_RES_CHECK", "1", 1);
        mtl_init();
        id<MTLDevice> device = MTLCreateSystemDefaultDevice(); assert(device);
        MTLTextureDescriptor *desc = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatBGRA8Unorm
            width:160 height:90 mipmapped:NO];
        desc.usage = MTLTextureUsageShaderRead;
        desc.storageMode = MTLStorageModeShared;
        id<MTLTexture> texture = [device newTextureWithDescriptor:desc]; assert(texture);
        uint32_t pixels[160 * 90];
        for (int i = 0; i < 160 * 90; i++) pixels[i] = 0xff3366cc;
        [texture replaceRegion:MTLRegionMake2D(0, 0, 160, 90) mipmapLevel:0 withBytes:pixels bytesPerRow:160 * 4];
        uint64_t serial = mtl_ext_tex((__bridge void *)texture);
        id<MTLSharedEvent> event = [device newSharedEvent];
        // Warm the presentation pipeline and allow the one-slot local encoder
        // to release its first lease before checking the queue dependency.
        event.signaledValue = 1;
        nvn_be_present_wait(mtl_present_ext(serial, 1.0 / 30, (__bridge void *)event, 1, NULL));
        usleep(250000);
        uint64_t start = clock_gettime_nsec_np(CLOCK_MONOTONIC);
        uint64_t ticket = mtl_present_ext(serial, 1.0 / 30, (__bridge void *)event, 2, NULL);
        // Local capture has one GPU slot. A later dropped frame must not move
        // the source-release watermark past this still-pending GPU copy.
        if (pressure) ticket = mtl_present_ext(serial, 1.0 / 30, (__bridge void *)event, 2, NULL);
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 300 * NSEC_PER_MSEC),
                       dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{ event.signaledValue = 2; });
        nvn_be_present_wait(ticket);
        uint64_t elapsed = clock_gettime_nsec_np(CLOCK_MONOTONIC) - start;
        if (elapsed < 250000000ull) {
            // Unblock the GPU before reporting failure; never abandon a queue
            // waiting on an external event in this negative control.
            event.signaledValue = 2;
            nvn_be_drain();
            nxb_video_probe_shutdown();
            fprintf(stderr, "FAIL: source released before producer completion (%.1f ms)\n", elapsed / 1e6);
            return 2;
        }
        for (int frame = 0; frame < 12; frame++) {
            usleep(100000);
            nvn_be_present_wait(mtl_present_ext(serial, 1.0 / 30, (__bridge void *)event, 2, NULL));
        }
        nvn_be_drain();
        nxb_video_probe_shutdown();
        struct stat st;
        assert(stat(argv[1], &st) == 0 && st.st_size > 1000);
        printf("PASS: headless S2Arm -> IOSurface -> H.264; producer wait %.1f ms, bytes %lld\n",
               elapsed / 1e6, (long long)st.st_size);
    }
    return 0;
}
