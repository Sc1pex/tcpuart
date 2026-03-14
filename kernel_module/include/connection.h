#ifndef _CONNECTION_H
#define _CONNECTION_H

#include <linux/tty.h>
#include <linux/workqueue.h>
#include <net/sock.h>
#include "state.h"
#include "tcpuart.h"

enum ConnectionFlags {
    CONN_ACTIVE,
    CONN_CONNECTED,
};

struct connection {
    int minor;

    struct socket* sock;
    void (*old_data_ready)(struct sock* sk);
    uint32_t sock_addr;
    uint16_t sock_port;

    unsigned long flags;

    struct work_struct rx_work;
    struct tty_driver* driver;
    struct tty_port port;
};

int conn_init(
    struct connection* conn, int minor, uint32_t addr, uint16_t port, struct tty_driver* driver
);

int conn_avabile(struct connection* conn);
void conn_destroy(struct connection* conn);

int conn_write(struct connection* conn, const unsigned char* buf, size_t count);
unsigned int conn_write_room(struct connection* conn);

int conn_get_info(struct connection* conn, struct tcpuart_server_info* info);

#endif
