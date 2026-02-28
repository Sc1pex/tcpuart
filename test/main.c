#include <arpa/inet.h>
#include <fcntl.h>
#include <stdio.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include "tcpuart.h"

int main() {
    int fd = open("/dev/tcpuart0", O_RDWR);
    if (fd < 0) {
        fprintf(stderr, "Faield to open device\n");
        return -1;
    }

    struct in_addr addr;
    inet_pton(AF_INET, "127.0.0.1", &addr);
    struct tcpuart_connect_to conn = { .addr = addr.s_addr, .port = 15113 };
    ioctl(fd, TCPUART_CONNECT_TO, &conn);

    printf("sent ioctl to device");
}
