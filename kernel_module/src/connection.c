#include "connection.h"

static int create_tcp_socket(struct socket** sock, uint32_t addr, uint16_t port) {
    int rc = sock_create_kern(&init_net, AF_INET, SOCK_STREAM, IPPROTO_TCP, sock);
    if (rc) {
        return rc;
    }

    struct sockaddr_in saddr = {
        .sin_family = AF_INET,
        .sin_addr.s_addr = addr,
        .sin_port = port,
    };

    rc = kernel_connect(*sock, (struct sockaddr*) &saddr, sizeof(saddr), 0);
    if (rc) {
        sock_release(*sock);
        *sock = NULL;
        return rc;
    }

    return 0;
}

int conn_init(
    struct connection* conn, int minor, uint32_t addr, uint16_t port,
    const struct tcpuart_state* state
) {
    conn->tcpuart_class = state->tcpuart_class;

    conn->minor = minor;
    conn->major = MAJOR(state->base_dev_num);

    // Try to connect to the socket
    int rc = create_tcp_socket(&conn->sock, addr, port);
    if (rc) {
        pr_err("failed to connect to tcp server\n");
        return rc;
    }

    dev_t new_dev = MKDEV(MAJOR(state->base_dev_num), conn->minor);
    cdev_init(&conn->cdev, &state->conn_fops);
    if (cdev_add(&conn->cdev, new_dev, 1)) {
        pr_err("failed to add cdev for conn\n");
        sock_release(conn->sock);
        return -ENOMEM;
    }

    conn->device =
        device_create(state->tcpuart_class, NULL, new_dev, NULL, "tcpuart%d", conn->minor);
    if (IS_ERR(conn->device)) {
        pr_err("failed to create device for minor %d\n", conn->minor);
        cdev_del(&conn->cdev);
        sock_release(conn->sock);
        return PTR_ERR(conn->device);
    }

    atomic_set(&conn->disconnected, false);
    atomic_set(&conn->refcount, 1);

    return 0;
}

void conn_init_empty(struct connection* conn) {
    atomic_set(&conn->disconnected, true);
    mutex_init(&conn->read_mutex);
}

int conn_avabile(struct connection* conn) {
    return atomic_read(&conn->refcount) == 0;
}

int conn_alive(struct connection* conn) {
    if (atomic_read(&conn->refcount)) {
        return !atomic_read(&conn->disconnected);
    }
    return false;
}

ssize_t conn_read(struct connection* conn, size_t count, char __user* dest_buf, int no_block) {
    mutex_lock(&conn->read_mutex);

    // First check if there is data left in the conn buffer
    if (conn->read_data_buf_len) {
        pr_info("Sending left ovevr data\n");
        ssize_t send_cnt = min(count, conn->read_data_buf_len);
        if (copy_to_user(dest_buf, conn->read_data_buf, send_cnt)) {
            mutex_unlock(&conn->read_mutex);
            return -EFAULT;
        }

        memmove(
            conn->read_data_buf, conn->read_data_buf + send_cnt, conn->read_data_buf_len - send_cnt
        );
        conn->read_data_buf_len -= send_cnt;

        mutex_unlock(&conn->read_mutex);
        return send_cnt;
    }

    if (atomic_read(&conn->disconnected)) {
        mutex_unlock(&conn->read_mutex);
        // No more data will be coming in, return 0 to indicate EOF
        return 0;
    }

    // No data in the buffer read from socket until we get a data message
    struct MessageHeader hdr;
    do {
        pr_info("Reading from socket\n");
        int ret = recv_message(&hdr, conn->read_data_buf, conn->sock, no_block);
        if (ret) {
            mutex_unlock(&conn->read_mutex);

            if (ret == -EAGAIN) {
                return ret;
            } else if (ret == -ECONNRESET || ret == -EPIPE || ret == -ESHUTDOWN
                       || ret == -ETIMEDOUT) {
                pr_info("Socket was closed by peer\n");
                conn_disconnect(conn);

                return 0;
            }

            pr_err("Failed to receive message: %d\n", ret);
            return ret;
        }
        conn->read_data_buf_len = hdr.size;
    } while (hdr.kind != MESSAGE_KIND_DATA);

    pr_info("Received data message of size: %zu\n", conn->read_data_buf_len);
    ssize_t send_cnt = min(count, conn->read_data_buf_len);
    if (copy_to_user(dest_buf, conn->read_data_buf, send_cnt)) {
        mutex_unlock(&conn->read_mutex);
        return -EFAULT;
    }

    memmove(
        conn->read_data_buf, conn->read_data_buf + send_cnt, conn->read_data_buf_len - send_cnt
    );
    conn->read_data_buf_len -= send_cnt;

    mutex_unlock(&conn->read_mutex);
    return send_cnt;
}

int conn_write(struct connection* conn, size_t count, char* buf) {
    if (atomic_read(&conn->disconnected)) {
        return -EPIPE;
    }

    struct MessageHeader hdr = {
        .kind = MESSAGE_KIND_DATA,
        .size = count,
    };

    int ret = send_message(hdr, buf, conn->sock);

    if (ret) {
        if (ret == -ECONNRESET || ret == -EPIPE || ret == -ESHUTDOWN || ret == -ETIMEDOUT) {
            pr_info("Socket was closed by peer\n");
            conn_disconnect(conn);
            return ret;
        }

        return ret;
    }
    return 0;
}

int conn_open(struct connection* conn) {
    if (!atomic_inc_not_zero(&conn->refcount)) {
        return -ENOTCONN;
    }
    return 0;
}

void conn_close(struct connection* conn) {
    if (atomic_dec_and_test(&conn->refcount)) {
        conn_destroy(conn);
    }
}

void conn_disconnect(struct connection* conn) {
    if (atomic_cmpxchg(&conn->disconnected, false, true) == false) {
        kernel_sock_shutdown(conn->sock, SHUT_RDWR);
        cdev_del(&conn->cdev);
        device_destroy(conn->tcpuart_class, MKDEV(conn->major, conn->minor));

        if (atomic_dec_and_test(&conn->refcount)) {
            conn_destroy(conn);
        }
    }
}

void conn_destroy(struct connection* conn) {
    sock_release(conn->sock);
    conn->sock = NULL;
    conn->read_data_buf_len = 0;
}

int conn_get_info(struct connection* conn, struct tcpuart_server_info* info) {
    if (atomic_read(&conn->disconnected)) {
        return -ENOTCONN;
    }

    struct sockaddr_in saddr;
    int ret = kernel_getpeername(conn->sock, (struct sockaddr*) &saddr);
    if (ret < 0) {
        return ret;
    }

    info->addr = saddr.sin_addr.s_addr;
    info->port = saddr.sin_port;

    return 0;
}
