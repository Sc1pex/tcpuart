#ifndef _CONNECTION_H
#define _CONNECTION_H

#include <linux/cdev.h>
#include <linux/fs.h>
#include <net/sock.h>
#include "message.h"
#include "state.h"

struct connection {
    struct cdev cdev;
    struct device* device;
    struct class* tcpuart_class;

    int minor;
    int major;

    struct socket* sock;

    uint8_t read_data_buf[MAXIMUM_MESSAGE_SIZE];
    size_t read_data_buf_len;

    int disconnected;
    atomic_t open_count;
};

#define CONN_DELETED 1

int conn_create(
    struct connection** conn, int minor, uint32_t addr, uint16_t port,
    const struct tcpuart_state* state
);

ssize_t conn_read(struct connection* conn, size_t count, char __user* dest_buf, int no_block);
// Count must be < MAXIMUM_MESSAGE_SIZE
int conn_write(struct connection* conn, size_t count, char* buf);

void conn_open(struct connection* conn);
int conn_close(struct connection* conn);

int conn_disconnect(struct connection* conn);
void conn_destroy(struct connection* conn);

#endif
