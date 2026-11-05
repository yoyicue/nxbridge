/* Proof of a real Mach send right transferring an IOSurface to a child/parent.
 * The registering process is this short-lived test, not the agent or game. */
#import <Foundation/Foundation.h>
#import <IOSurface/IOSurface.h>
#include <mach/mach.h>
#include <mach/mach_init.h>
#include <spawn.h>
#include <sys/wait.h>
#include <unistd.h>
#include <assert.h>
#include <stdio.h>

extern char **environ;
typedef struct {
    mach_msg_header_t header;
    mach_msg_body_t body;
    mach_msg_port_descriptor_t surface;
    uint32_t marker;
    mach_msg_trailer_t trailer;
} message;

static int child(void) {
    mach_port_array_t ports = NULL; mach_msg_type_number_t count = 0;
    assert(mach_ports_lookup(mach_task_self(), &ports, &count) == KERN_SUCCESS);
    assert(count > 1 && MACH_PORT_VALID(ports[1]));
    mach_port_t parent = ports[1];
    NSDictionary *props = @{(__bridge NSString *)kIOSurfaceWidth: @4,
                            (__bridge NSString *)kIOSurfaceHeight: @4,
                            (__bridge NSString *)kIOSurfaceBytesPerElement: @4};
    IOSurfaceRef surface = IOSurfaceCreate((__bridge CFDictionaryRef)props); assert(surface);
    assert(IOSurfaceLock(surface, 0, NULL) == kIOReturnSuccess);
    ((uint8_t *)IOSurfaceGetBaseAddress(surface))[0] = 0x5a;
    assert(IOSurfaceUnlock(surface, 0, NULL) == kIOReturnSuccess);
    mach_port_t port = IOSurfaceCreateMachPort(surface); assert(MACH_PORT_VALID(port));
    message m = {0};
    m.header.msgh_bits = MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND, 0) | MACH_MSGH_BITS_COMPLEX;
    m.header.msgh_size = (mach_msg_size_t)offsetof(message, trailer);
    m.header.msgh_remote_port = parent;
    m.header.msgh_id = 35001;
    m.body.msgh_descriptor_count = 1;
    m.surface.name = port; m.surface.disposition = MACH_MSG_TYPE_COPY_SEND; m.surface.type = MACH_MSG_PORT_DESCRIPTOR;
    m.marker = 0x1234abcd;
    assert(mach_msg(&m.header, MACH_SEND_MSG, m.header.msgh_size, 0, MACH_PORT_NULL, MACH_MSG_TIMEOUT_NONE, MACH_PORT_NULL) == MACH_MSG_SUCCESS);
    mach_port_deallocate(mach_task_self(), port);
    CFRelease(surface);
    for (mach_msg_type_number_t i = 0; i < count; i++) if (MACH_PORT_VALID(ports[i])) mach_port_deallocate(mach_task_self(), ports[i]);
    vm_deallocate(mach_task_self(), (vm_address_t)ports, count * sizeof(*ports));
    return 0;
}

int main(int argc, char **argv) {
    @autoreleasepool {
        if (argc == 2 && !strcmp(argv[1], "child")) return child();
        mach_port_t receive = MACH_PORT_NULL;
        assert(mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &receive) == KERN_SUCCESS);
        assert(mach_port_insert_right(mach_task_self(), receive, receive, MACH_MSG_TYPE_MAKE_SEND) == KERN_SUCCESS);
        mach_port_array_t old = NULL; mach_msg_type_number_t old_count = 0;
        assert(mach_ports_lookup(mach_task_self(), &old, &old_count) == KERN_SUCCESS);
        assert(old_count <= 3);
        mach_port_t passed[3] = {MACH_PORT_NULL, MACH_PORT_NULL, MACH_PORT_NULL};
        for (unsigned i = 0; i < old_count; i++) passed[i] = old[i];
        assert(!MACH_PORT_VALID(passed[1]));
        passed[1] = receive;
        assert(mach_ports_register(mach_task_self(), passed, 3) == KERN_SUCCESS);
        pid_t pid = -1; char *args[] = {argv[0], "child", NULL};
        int result = posix_spawn(&pid, argv[0], NULL, NULL, args, environ);
        assert(mach_ports_register(mach_task_self(), old, old_count) == KERN_SUCCESS);
        assert(result == 0);
        message m = {0};
        assert(mach_msg(&m.header, MACH_RCV_MSG | MACH_RCV_TIMEOUT, 0, sizeof m, receive, 10000, MACH_PORT_NULL) == MACH_MSG_SUCCESS);
        assert(m.header.msgh_id == 35001 && m.body.msgh_descriptor_count == 1 && m.marker == 0x1234abcd);
        IOSurfaceRef shared = IOSurfaceLookupFromMachPort(m.surface.name); assert(shared);
        assert(IOSurfaceLock(shared, kIOSurfaceLockReadOnly, NULL) == kIOReturnSuccess);
        assert(((const uint8_t *)IOSurfaceGetBaseAddress(shared))[0] == 0x5a);
        assert(IOSurfaceUnlock(shared, kIOSurfaceLockReadOnly, NULL) == kIOReturnSuccess);
        CFRelease(shared); mach_port_deallocate(mach_task_self(), m.surface.name);
        int status = 0; assert(waitpid(pid, &status, 0) == pid && WIFEXITED(status) && WEXITSTATUS(status) == 0);
        for (mach_msg_type_number_t i = 0; i < old_count; i++) if (MACH_PORT_VALID(old[i])) mach_port_deallocate(mach_task_self(), old[i]);
        vm_deallocate(mach_task_self(), (vm_address_t)old, old_count * sizeof(*old));
        assert(mach_port_deallocate(mach_task_self(), receive) == KERN_SUCCESS);
        assert(mach_port_mod_refs(mach_task_self(), receive, MACH_PORT_RIGHT_RECEIVE, -1) == KERN_SUCCESS);
        puts("IOSurface Mach right inherited and transferred across processes");
    }
    return 0;
}
