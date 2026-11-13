/* Experimental scheduling of independent page hashes. The caller owns the
 * source and output for the entire synchronous call. This does not implement
 * the hash, write tracking, synchronization or the texture upload itself. */
#ifndef NXBRIDGE_TEXTURE_HASH_PARALLEL_H
#define NXBRIDGE_TEXTURE_HASH_PARALLEL_H

#include <dispatch/dispatch.h>
#include <stdint.h>

typedef void (*nxb_hash_page_fn)(void *context, uint32_t page);
typedef struct { void *context; nxb_hash_page_fn page; uint32_t count; } nxb_hash_job;

static void nxb_hash_chunk(void *context, size_t part) {
    nxb_hash_job *job = context;
    uint32_t begin = (uint32_t)(((uint64_t)job->count * part) / 4);
    uint32_t end = (uint32_t)(((uint64_t)job->count * (part + 1)) / 4);
    for (uint32_t page = begin; page < end; page++) job->page(job->context, page);
}

static inline void nxb_hash_pages(uint32_t count, void *context, nxb_hash_page_fn page) {
    if (count < 32) {
        for (uint32_t i = 0; i < count; i++) page(context, i);
        return;
    }
    nxb_hash_job job = {context, page, count};
    /* dispatch_apply_f returns only after every chunk is complete. */
    dispatch_apply_f(4, dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), &job, nxb_hash_chunk);
}
#endif
