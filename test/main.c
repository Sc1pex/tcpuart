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
    /* inet_pton(AF_INET, "192.168.0.97", &addr); */
    inet_pton(AF_INET, "127.0.0.1", &addr);
    struct tcpuart_connect_to conn = { .addr = addr.s_addr, .port = htons(15113) };
    int fileid = ioctl(fd, TCPUART_CONNECT_TO, &conn);

    char filename[64];
    snprintf(filename, 64, "/dev/tcpuart%d", fileid);

    printf("Created node: %s\n", filename);

    int connfd = open(filename, O_RDWR);
    char buf[64];

    strcpy(buf, "abcdef");
    int n = write(connfd, buf, 6);
    printf("wrote %d bytes\n", n);

    for (;;) {
        int n = read(connfd, buf, 2);
        printf("read %d bytes\n", n);

        if (n <= 0) {
            printf("Closing\n");
            break;
        }

        buf[n] = 0;
        printf("data: %s\n", buf);
    }

    close(connfd);
}
