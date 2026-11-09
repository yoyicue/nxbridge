/* Experimental, per-process controller receiver. Disabled unless both environment
 * variables are set. The TCP listener is loopback-only, normally reached through
 * an explicitly selected device's adb reverse. Protocol and token are private to
 * one experiment; the network listener is not a general LAN pairing service. */
#include "nxbridge_input.h"
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <unistd.h>
#include <time.h>

static pthread_mutex_t mu = PTHREAD_MUTEX_INITIALIZER;
/* Preserve button transitions in order until the runtime has sampled them.
 * Axis changes alone stay latest-only, so ordinary movement does not queue. */
enum { EDGE_CAPACITY = 8 };
struct input_snapshot { uint16_t buttons; int32_t axes[4]; };
static struct input_snapshot edges[EDGE_CAPACITY];
static unsigned edge_head, edge_count;
static uint16_t held;
static int32_t axis[4];
static int overflow_quarantine;
static unsigned char token[32];
static int listener = -1;
/* Configuring a session reserves player 0 for that session, even if its
 * listener fails to start. Unassigned host controllers must not leak in. */
static int isolated_input;
static FILE *trace;
static int trace_monotonic;

static void clear_state(void) {
    pthread_mutex_lock(&mu);
    held = 0; edge_head = edge_count = 0; overflow_quarantine = 0;
    memset(axis, 0, sizeof axis);
    pthread_mutex_unlock(&mu);
}
void nxbridge_input_sample(uint64_t *buttons, int32_t out[4]) {
    if (!isolated_input) return;
    pthread_mutex_lock(&mu);
    uint16_t sample_buttons = held;
    const int32_t *sample_axis = axis;
    if (edge_count) {
        struct input_snapshot *edge = &edges[edge_head];
        sample_buttons = edge->buttons; sample_axis = edge->axes;
        edge_head = (edge_head + 1) % EDGE_CAPACITY; edge_count--;
    }
    *buttons = sample_buttons;
    memcpy(out, sample_axis, sizeof axis);
    pthread_mutex_unlock(&mu);
}
void nxbridge_input_trace(uint64_t buttons, const int32_t axes[4]) {
    if (!trace) return;
    static uint64_t previous = UINT64_MAX;
    static int32_t prior[4];
    pthread_mutex_lock(&mu);
    if (buttons != previous || memcmp(prior, axes, sizeof prior)) {
        if (trace_monotonic) {
            struct timespec now;
            if (clock_gettime(CLOCK_MONOTONIC, &now) == 0)
                fprintf(trace, "M %llu ", (unsigned long long)now.tv_sec * 1000000000ULL + (unsigned long long)now.tv_nsec);
            else
                fprintf(trace, "M unknown ");
        }
        fprintf(trace, "%llu %d %d %d %d\n", (unsigned long long)buttons, axes[0], axes[1], axes[2], axes[3]);
        fflush(trace); previous = buttons; memcpy(prior, axes, sizeof prior);
    }
    pthread_mutex_unlock(&mu);
}
/* A complete-frame deadline is never extended by a slow byte stream. The
 * ordinary idle window stays at one second for heartbeat jitter; after the
 * first byte of a frame arrives, a stalled fragment gets a shorter deadline. */
#define FRAME_IDLE_TIMEOUT_MS 1000
#define FRAME_FRAGMENT_TIMEOUT_MS 200
static int read_exact(int fd, unsigned char *p, size_t n, int fragment_timeout_ms) {
    struct timespec now, deadline;
    if (clock_gettime(CLOCK_MONOTONIC, &deadline) != 0) return 0;
    deadline.tv_sec += FRAME_IDLE_TIMEOUT_MS / 1000;
    int fragment_deadline_set = 0;
    while (n) {
        if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) return 0;
        int64_t us = (deadline.tv_sec - now.tv_sec) * 1000000LL + (deadline.tv_nsec - now.tv_nsec) / 1000;
        if (us <= 0) return 0;
        fd_set set; FD_ZERO(&set); FD_SET(fd, &set);
        struct timeval tv = {(long)(us / 1000000), (int)(us % 1000000)};
        int ready = select(fd + 1, &set, 0, 0, &tv);
        if (ready <= 0) return 0;
        ssize_t got = recv(fd, p, n, 0);
        if (got <= 0) return 0;
        p += got; n -= (size_t)got;
        if (n && fragment_timeout_ms > 0 && !fragment_deadline_set) {
            if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) return 0;
            now.tv_nsec += (long)fragment_timeout_ms * 1000000L;
            now.tv_sec += now.tv_nsec / 1000000000L;
            now.tv_nsec %= 1000000000L;
            if (now.tv_sec < deadline.tv_sec ||
                (now.tv_sec == deadline.tv_sec && now.tv_nsec < deadline.tv_nsec)) deadline = now;
            fragment_deadline_set = 1;
        }
    }
    return 1;
}
static uint64_t be64(const unsigned char *p) {
    uint64_t x = 0; for (int i = 0; i < 8; i++) x = x * 256 + p[i]; return x;
}
static void serve(int fd) {
    unsigned char auth[32], f[22]; uint64_t last = 0;
    if (!read_exact(fd, auth, sizeof auth, 0)) return;
    unsigned diff = 0; for (int i = 0; i < 32; i++) diff |= auth[i] ^ token[i];
    if (diff) return;
    while (read_exact(fd, f, sizeof f, FRAME_FRAGMENT_TIMEOUT_MS)) {
        int reset = memcmp(f, "NXIR", 4) == 0;
        if (!reset && memcmp(f, "NXI1", 4)) break;
        uint64_t seq = be64(f + 4);
        if (seq <= last) break;
        int32_t a[4];
        for (int i = 0; i < 4; i++) {
            uint16_t v = (uint16_t)((unsigned)f[14 + 2*i] << 8 | f[15 + 2*i]);
            a[i] = v >= 32768 ? (int32_t)v - 65536 : v;
            if (a[i] == -32768) a[i] = -32767;
        }
        uint16_t b = (uint16_t)((unsigned)f[12] << 8 | f[13]);
        if (reset) {
            int neutral = b == 0;
            for (int i = 0; i < 4; i++) if (a[i]) neutral = 0;
            if (!neutral) break;
            clear_state(); last = seq; continue;
        }
        pthread_mutex_lock(&mu);
        if (overflow_quarantine) {
            int neutral = b == 0;
            for (int i = 0; i < 4; i++) if (a[i]) neutral = 0;
            if (neutral) overflow_quarantine = 0;
        } else if (b != held && edge_count == EDGE_CAPACITY) {
            /* Never silently lose a release. Require a fresh neutral snapshot. */
            held = 0; edge_head = edge_count = 0; memset(axis, 0, sizeof axis);
            overflow_quarantine = 1;
            fprintf(stderr, "NXBridge input: edge queue overflow; neutralized until release\n");
        } else {
            if (b != held) {
                unsigned tail = (edge_head + edge_count) % EDGE_CAPACITY;
                edges[tail].buttons = b; memcpy(edges[tail].axes, a, sizeof a);
                edge_count++;
            }
            held = b; memcpy(axis, a, sizeof axis);
        }
        pthread_mutex_unlock(&mu);
        last = seq;
    }
    clear_state();
}
static void *run(void *unused) {
    (void)unused;
    for (;;) {
        int fd = accept(listener, 0, 0);
        if (fd < 0) { if (errno == EINTR) continue; return 0; }
        serve(fd); close(fd);
        /* Never promote a connection queued during an existing session. */
        int flags = fcntl(listener, F_GETFL);
        fcntl(listener, F_SETFL, flags | O_NONBLOCK);
        while ((fd = accept(listener, 0, 0)) >= 0) close(fd);
        fcntl(listener, F_SETFL, flags);
    }
}
static int hexval(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}
__attribute__((constructor)) static void start(void) {
    const char *key = getenv("NXBRIDGE_INPUT_TOKEN"), *port = getenv("NXBRIDGE_INPUT_PORT");
    if (!key && !port) return;
    isolated_input = 1;
    if (!key || strlen(key) != 64 || !port) { fprintf(stderr, "NXBridge input: invalid configuration\n"); return; }
    for (int i = 0; i < 32; i++) {
        int h = hexval(key[2*i]), l = hexval(key[2*i+1]);
        if (h < 0 || l < 0) { fprintf(stderr, "NXBridge input: invalid token\n"); return; }
        token[i] = (unsigned char)(h * 16 + l);
    }
    char *end; long p = strtol(port, &end, 10);
    if (*end || p < 1024 || p > 65535) { fprintf(stderr, "NXBridge input: invalid port\n"); return; }
    listener = socket(AF_INET, SOCK_STREAM, 0);
    if (listener < 0) return;
    struct sockaddr_in address = {0}; address.sin_family = AF_INET;
    address.sin_addr.s_addr = htonl(0x7f000001u); address.sin_port = htons((uint16_t)p);
    if (bind(listener, (struct sockaddr *)&address, sizeof address) || listen(listener, 1)) {
        perror("NXBridge input: bind/listen"); close(listener); listener = -1; return;
    }
    const char *trace_path = getenv("NXBRIDGE_INPUT_TRACE");
    if (trace_path) {
        const char *format = getenv("NXBRIDGE_INPUT_TRACE_MONOTONIC");
        trace_monotonic = format && !strcmp(format, "1");
        trace = fopen(trace_path, "a");
    }
    pthread_t t;
    if (pthread_create(&t, 0, run, 0)) { close(listener); listener = -1; return; }
    pthread_detach(t);
    fprintf(stderr, "NXBridge input: loopback listener ready\n");
}
