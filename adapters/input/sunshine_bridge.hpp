#pragma once
/* NXBridge single-worker adapter for Sunshine's already-authenticated input_t.
 * Wire format is local to the host; network input remains Moonlight/GameStream. */
#include <algorithm>
#include <array>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <fstream>
#include <iterator>
#include <mutex>
#include <string>
#include <thread>
#include <arpa/inet.h>
#include <sys/socket.h>
#include <unistd.h>

namespace nxbridge {
struct state { std::uint16_t buttons = 0; std::array<std::int16_t,4> axes {}; };
inline bool neutral(const state &s) {
    return s.buttons == 0 && std::all_of(s.axes.begin(), s.axes.end(), [](auto x) { return x == 0; });
}
inline state convert(std::uint16_t flags, std::uint8_t lt, std::uint8_t rt,
                     std::int16_t lx, std::int16_t ly, std::int16_t rx, std::int16_t ry) {
    state s;
    constexpr std::array<std::uint16_t,14> sources {0x1000,0x2000,0x4000,0x8000,0x0040,0x0080,
        0x0100,0x0200,0x0010,0x0020,0x0004,0x0001,0x0008,0x0002};
    constexpr std::array<int,14> dest {0,1,2,3,4,5,6,7,10,11,12,13,14,15};
    for (unsigned i = 0; i < sources.size(); i++) if (flags & sources[i]) s.buttons |= 1u << dest[i];
    if (lt >= 128) s.buttons |= 1u << 8;
    if (rt >= 128) s.buttons |= 1u << 9;
    s.axes = {lx,ly,rx,ry};
    return s;
}
inline bool configured() {
    return std::getenv("NXBRIDGE_INPUT_PORT") || std::getenv("NXBRIDGE_INPUT_TOKEN") || std::getenv("NXBRIDGE_CLIENT_CERT");
}
inline bool allowed(const std::string &authenticated_client_pem) {
    const char *path = std::getenv("NXBRIDGE_CLIENT_CERT");
    if (!path || authenticated_client_pem.empty()) return false;
    std::ifstream cert(path, std::ios::binary);
    if (!cert) return false;
    std::string expected((std::istreambuf_iterator<char>(cert)), {});
    return !expected.empty() && authenticated_client_pem == expected;
}
inline int hex(char c) {
    if (c >= '0' && c <= '9') return c-'0';
    if (c >= 'a' && c <= 'f') return c-'a'+10;
    if (c >= 'A' && c <= 'F') return c-'A'+10;
    return -1;
}
class sender {
    std::mutex mu;
    std::condition_variable changed;
    std::deque<state> queue;
    std::thread worker;
    std::array<unsigned char,32> token {};
    state current;
    int port = 0;
    int fd = -1;
    bool running = false, connected = false, overflow = false, ever_connected = false, await_neutral = false;
    static bool write_all(int s, const unsigned char *p, size_t n) {
        while (n) {
#ifdef MSG_NOSIGNAL
            auto sent = ::send(s, p, n, MSG_NOSIGNAL);
#else
            auto sent = ::send(s, p, n, 0);
#endif
            if (sent <= 0) return false;
            p += sent; n -= static_cast<size_t>(sent);
        }
        return true;
    }
    static std::array<unsigned char,22> frame(std::uint64_t sequence, const state &s) {
        std::array<unsigned char,22> b {}; b[0]='N';b[1]='X';b[2]='I';b[3]='1';
        for (int i=0;i<8;i++) b[4+i]=static_cast<unsigned char>(sequence >> (56-8*i));
        b[12]=static_cast<unsigned char>(s.buttons >> 8);b[13]=static_cast<unsigned char>(s.buttons);
        for (int i=0;i<4;i++) { auto v=static_cast<std::uint16_t>(s.axes[i]); b[14+2*i]=static_cast<unsigned char>(v>>8); b[15+2*i]=static_cast<unsigned char>(v); }
        return b;
    }
    void run() {
        using namespace std::chrono_literals;
        while (true) {
            { std::lock_guard lock(mu); if (!running) break; }
            int s = ::socket(AF_INET, SOCK_STREAM, 0);
            if (s >= 0) {
#ifdef SO_NOSIGPIPE
                int yes=1; setsockopt(s,SOL_SOCKET,SO_NOSIGPIPE,&yes,sizeof yes);
#endif
                timeval tv {1,0}; setsockopt(s,SOL_SOCKET,SO_SNDTIMEO,&tv,sizeof tv);
                sockaddr_in address {};address.sin_family=AF_INET;address.sin_port=htons(static_cast<std::uint16_t>(port));address.sin_addr.s_addr=htonl(0x7f000001u);
                if (::connect(s,reinterpret_cast<sockaddr*>(&address),sizeof address) || !write_all(s,token.data(),token.size())) { ::close(s); s=-1; }
            }
            if (s < 0) {
                std::unique_lock lock(mu); changed.wait_for(lock,1s,[&]{return !running;}); continue;
            }
            {
                std::lock_guard lock(mu);
                if (!running) { ::close(s); break; }
                fd=s; connected=true; queue.clear(); current={}; overflow=false;
                await_neutral=ever_connected; ever_connected=true;
            }
            std::fprintf(stderr,"NXBridge: authenticated stream input connected to game\n");
            std::uint64_t sequence=0;
            for (;;) {
                state next;
                {
                    std::unique_lock lock(mu);
                    changed.wait_for(lock,200ms,[&]{return !running || overflow || !queue.empty();});
                    if (!running || overflow) break;
                    if (!queue.empty()) { current=queue.front();queue.pop_front(); }
                    next=current;
                }
                auto bytes=frame(++sequence,next);
                if (!write_all(s,bytes.data(),bytes.size())) break;
            }
            {
                std::lock_guard lock(mu);
                connected=false;fd=-1;queue.clear();current={};
            }
            ::close(s);
        }
    }
public:
    sender() {
        const char *p=std::getenv("NXBRIDGE_INPUT_PORT"), *key=std::getenv("NXBRIDGE_INPUT_TOKEN");
        if (!p || !key || std::strlen(key)!=64) return;
        char *end=nullptr;long v=std::strtol(p,&end,10);if (!end || *end || v<1024 || v>65535) return;
        for (int i=0;i<32;i++) {int a=hex(key[2*i]),b=hex(key[2*i+1]);if(a<0||b<0)return;token[i]=static_cast<unsigned char>(a*16+b);}
        port=static_cast<int>(v);
    }
    sender(const sender&) = delete;
    sender& operator=(const sender&) = delete;
    ~sender() { stop(); }
    void start() {
        std::lock_guard lock(mu);
        if (!port || running) return;
        running=true;ever_connected=false;await_neutral=false;
        worker=std::thread([this]{run();});
    }
    void stop() {
        {
            std::lock_guard lock(mu);
            if (!running) return;
            running=false;connected=false;queue.clear();current={};
            if (fd>=0) ::shutdown(fd,SHUT_RDWR);
        }
        changed.notify_all();
        if (worker.joinable()) worker.join();
    }
    void update(const state &s) {
        std::lock_guard lock(mu);
        if (!running || !connected) return;
        if (await_neutral) { if (neutral(s)) await_neutral=false; return; }
        if (queue.size() >= 256) {
            overflow=true; connected=false;queue.clear();current={};
            if (fd>=0) ::shutdown(fd,SHUT_RDWR);
        } else queue.push_back(s);
        changed.notify_one();
    }
};
}
