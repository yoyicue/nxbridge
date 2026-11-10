/* SPDX-License-Identifier: MIT
 * Included by S2Arm's nvn_mtl.m on its existing presentation thread.
 * Use the rendering queue and its presentation slots, including residency,
 * external-producer waits and completion fences. No CPU texture readback.
 */
static int nxb_present_skipped(void) {
    // The upstream caller advances its completion watermark on return 0.
    // A skipped newer frame must not release an older external texture while
    // the GPU is still reading it. Wait on the presentation thread only.
    pthread_mutex_lock(&g_pq_mu);
    while (g_pq_inflight) pthread_cond_wait(&g_pq_cv, &g_pq_mu);
    pthread_mutex_unlock(&g_pq_mu);
    return 0;
}

static int nxb_present_offscreen(id<MTLTexture> src, const int crop[4], double period, uint64_t ticket) {
    nxb_probe_ticket capture;
    id<MTLTexture> out = nxb_video_probe_claim(g_dev, period, &capture);
    if (!out) return nxb_present_skipped();

    int slot = (int)(ticket % 3);
    present_pipeline(out.pixelFormat);
    present_options();
    pthread_mutex_lock(&g_pq_mu);
    while ((g_present_aa_slots[slot] && g_present_aa_slots[slot]->busy) ||
           (g_present_fx_slots[slot] && g_present_fx_slots[slot]->busy) ||
           g_pq_done < g_present_alloc_fence[slot])
        pthread_cond_wait(&g_pq_cv, &g_pq_mu);
    pthread_mutex_unlock(&g_pq_mu);

    id<MTLCommandBuffer> cb3 = nil;
    id<MTL4CommandBuffer> cb4 = nil;
    if (g_m3) {
        cb3 = [g_q3 commandBuffer];
        ext_wait3(cb3);
        if (!present_encode_slot(cb3, src, out, crop, slot)) {
            nxb_video_probe_complete(capture, 0);
            return nxb_present_skipped();
        }
    } else if (@available(macOS 26.0, iOS 26.0, *)) {
        [g_present_alloc4[slot] reset];
        cb4 = g_present_cb[slot];
        [cb4 beginCommandBufferWithAllocator:g_present_alloc4[slot]];
        int encoded = present_encode_m4(cb4, src, out, crop, slot);
        [cb4 endCommandBuffer];
        if (!encoded) {
            nxb_video_probe_complete(capture, 0);
            return nxb_present_skipped();
        }
    } else {
        nxb_video_probe_complete(capture, 0);
        return nxb_present_skipped();
    }

    NvPresentAA *aa = g_present_aa ? g_present_aa_slots[slot] : nil;
    NvPresentFX *fx = g_present_fx ? g_present_fx_slots[slot] : nil;
    id feedback = fb_new();
    fb_add(feedback, ^(id<MTL4CommitFeedback> result) {
        if (result.error && !mtl_gpu_cb_error(result.error, "NXBridge offscreen", ticket))
            MLOG("【红】 NXBridge offscreen command buffer: %s", result.error.description.UTF8String);
        nxb_video_probe_complete(capture, result.error == nil);
        pthread_mutex_lock(&g_pq_mu);
        if (aa) aa->busy = NO;
        if (fx) fx->busy = NO;
        if (ticket > g_pq_done) g_pq_done = ticket;
        g_pq_inflight--;
        pthread_cond_broadcast(&g_pq_cv);
        pthread_mutex_unlock(&g_pq_mu);
    });
    pthread_mutex_lock(&g_pq_mu);
    if (aa) aa->busy = YES;
    if (fx) fx->busy = YES;
    g_pq_inflight++;
    pthread_mutex_unlock(&g_pq_mu);
    res_add(out);
    res_commit();
    if (g_m3) m3_commit(cb3, feedback, nil, 0);
    else {
        id<MTL4CommandBuffer> buffers[] = {cb4};
        [g_q4 commit:buffers count:1 options:feedback];
    }
    g_present_alloc_fence[slot] = ticket;
    return 1;
}
