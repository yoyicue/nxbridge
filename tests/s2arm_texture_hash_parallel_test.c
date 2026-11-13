#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "../adapters/s2arm/texture_hash_parallel.h"

/* Exact hash used by the pinned upstream: results must not change. */
static uint64_t hash(const uint8_t *m, size_t n) {
    uint64_t h = 0x9e3779b97f4a7c15ull ^ n, w; size_t i = 0;
    for (; i + 8 <= n; i += 8) { memcpy(&w, m + i, 8); h = (h ^ w) * 0xff51afd7ed558ccdull; h ^= h >> 32; }
    for (; i < n; i++) h = (h ^ m[i]) * 0x100000001b3ull;
    return h;
}
typedef struct { const uint8_t *data; uint64_t *out; _Atomic unsigned *calls; size_t len, page, offset; } fixture;
static void one(void *context, uint32_t k) {
    fixture *f = context;
    size_t lo = k * f->page > f->offset ? k * f->page - f->offset : 0;
    size_t hi = (k + 1) * f->page - f->offset;
    if (hi > f->len) hi = f->len;
    f->out[k] = hash(f->data + lo, hi - lo);
    atomic_fetch_add(&f->calls[k], 1);
}
int main(void) {
    enum { MAX_PAGES = 259, PAGE = 16384 };
    uint8_t *data = malloc(MAX_PAGES * PAGE); uint64_t *actual = calloc(MAX_PAGES, sizeof *actual);
    _Atomic unsigned *calls = calloc(MAX_PAGES, sizeof *calls);
    if (!data || !actual || !calls) return 2;
    for (size_t i = 0; i < MAX_PAGES * PAGE; i++) data[i] = (uint8_t)(i * 47 + (i >> 11));
    const unsigned counts[] = {0, 1, 3, 31, 32, 33, 63, 127, 258};
    for (size_t c = 0; c < sizeof counts / sizeof *counts; c++) for (size_t offset = 0; offset < 3; offset++) {
        uint32_t n = counts[c]; size_t shift = offset ? (offset == 1 ? 1 : PAGE - 1) : 0;
        size_t len = n ? (size_t)n * PAGE - shift - (n > 1 ? 3 : 0) : 0;
        fixture f = {data, actual, calls, len, PAGE, shift};
        memset(actual, 0, MAX_PAGES * sizeof *actual);
        for (size_t i = 0; i < MAX_PAGES; i++) atomic_store(&calls[i], 0);
        nxb_hash_pages(n, &f, one);
        for (uint32_t i = 0; i < n; i++) {
            size_t lo = i * PAGE > shift ? i * PAGE - shift : 0;
            size_t hi = (i + 1) * PAGE - shift; if (hi > len) hi = len;
            if (atomic_load(&calls[i]) != 1 || actual[i] != hash(data + lo, hi - lo)) return 1;
        }
        if (n < MAX_PAGES && atomic_load(&calls[n]) != 0) return 1;
    }
    free(data); free(actual); free(calls); puts("page hash scheduling passed"); return 0;
}
