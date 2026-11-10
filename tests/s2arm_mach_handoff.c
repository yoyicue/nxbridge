/* SPDX-License-Identifier: MIT
 * Game-free proof that the S2Arm launcher preserves a real Mach send right.
 * --spawn registers a receive endpoint, then starts the launcher. The same
 * binary used as its synthetic host sends a message through inherited slot 1.
 */
#include <mach/mach.h>
#include <spawn.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>
extern char **environ;

int main(int argc, char **argv) {
    if (argc < 2 || strcmp(argv[1], "--spawn")) {
        mach_port_array_t ports = NULL;
        mach_msg_type_number_t count = 0;
        kern_return_t result = mach_ports_lookup(mach_task_self(), &ports, &count);
        if (result || count < 2 || !MACH_PORT_VALID(ports[1])) {
            fprintf(stderr, "inherited media endpoint missing after launcher preflight\n");
            return 2;
        }
        mach_msg_header_t message = {0};
        message.msgh_bits = MACH_MSGH_BITS(MACH_MSG_TYPE_COPY_SEND, 0);
        message.msgh_remote_port = ports[1];
        message.msgh_size = sizeof(message);
        message.msgh_id = 127;
        result = mach_msg(&message, MACH_SEND_MSG | MACH_SEND_TIMEOUT,
                          sizeof(message), 0, MACH_PORT_NULL, 100, MACH_PORT_NULL);
        for (unsigned i = 0; i < count; i++)
            if (MACH_PORT_VALID(ports[i])) mach_port_deallocate(mach_task_self(), ports[i]);
        vm_deallocate(mach_task_self(), (vm_address_t)ports, count * sizeof(*ports));
        return result ? 3 : 0;
    }
    if (argc < 3) return 4;
    mach_port_t receive = MACH_PORT_NULL;
    if (mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &receive) ||
        mach_port_insert_right(mach_task_self(), receive, receive, MACH_MSG_TYPE_MAKE_SEND)) return 5;
    mach_port_t ports[3] = {MACH_PORT_NULL, receive, MACH_PORT_NULL};
    if (mach_ports_register(mach_task_self(), ports, 3)) return 6;
    pid_t child;
    if (posix_spawn(&child, argv[2], NULL, NULL, argv + 2, environ)) return 7;
    int status;
    if (waitpid(child, &status, 0) != child || !WIFEXITED(status)) return 8;
    if (WEXITSTATUS(status)) return WEXITSTATUS(status);
    struct { mach_msg_header_t header; mach_msg_max_trailer_t trailer; } received = {0};
    kern_return_t result = mach_msg(&received.header, MACH_RCV_MSG | MACH_RCV_TIMEOUT,
        0, sizeof(received), receive, 1000, MACH_PORT_NULL);
    if (result || received.header.msgh_id != 127) return 9;
    puts("PASS: inherited media endpoint delivered the host message");
    return 0;
}
