#include <linux/ioctl.h>

#ifdef __KERNEL__
#include <linux/types.h>
#else
#include <stdint.h>
#endif

#define TCPUART_MAGIC 'T'

struct tcpuart_connect_to {
    uint32_t addr;
    uint16_t port;
};

#define TCPUART_CONNECT_TO _IOW(TCPUART_MAGIC, 0, struct tcpuart_connect_to)
