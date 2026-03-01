#include <arpa/inet.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <unistd.h>
#include "tcpuart.h"

int main() {
    int fd = open("/dev/tcpuart0", O_RDWR);
    if (fd < 0) {
        fprintf(stderr, "Faield to open device\n");
        return -1;
    }

    struct in_addr addr;
    inet_pton(AF_INET, "192.168.0.97", &addr);
    struct tcpuart_connect_to conn = { .addr = addr.s_addr, .port = htons(15113) };
    int fileid = ioctl(fd, TCPUART_CONNECT_TO, &conn);

    char filename[64];
    snprintf(filename, 64, "/dev/tcpuart%d", fileid);

    printf("Created node: %s\n", filename);

    int connfd = open(filename, O_RDWR);
    char buf[64];
    int n = read(connfd, buf, 64);
    printf("read %d bytes\n", n);

    strcpy(buf, "abcdef");
    n = write(connfd, buf, 6);
    printf("wrote %d bytes\n", n);

    close(connfd);
}
