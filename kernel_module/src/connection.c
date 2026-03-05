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

int conn_create(
    struct connection** p_conn, int minor, uint32_t addr, uint16_t port,
    const struct tcpuart_state* state
) {
    *p_conn = kzalloc(sizeof(**p_conn), GFP_KERNEL);
    if (!(*p_conn)) {
        return -ENOMEM;
    }
    struct connection* conn = *p_conn;

    conn->tcpuart_class = state->tcpuart_class;

    conn->minor = minor;
    conn->major = MAJOR(state->base_dev_num);

    atomic_set(&conn->open_count, 0);

    // Try to connect to the socket
    int rc = create_tcp_socket(&conn->sock, addr, port);
    if (rc) {
        pr_err("failed to connect to tcp server\n");
        kfree(conn);
        *p_conn = NULL;
        return rc;
    }

    dev_t new_dev = MKDEV(MAJOR(state->base_dev_num), conn->minor);
    cdev_init(&conn->cdev, &state->conn_fops);
    if (cdev_add(&conn->cdev, new_dev, 1)) {
        pr_err("failed to add cdev for conn\n");
        sock_release(conn->sock);
        kfree(conn);
        *p_conn = NULL;
        return -ENOMEM;
    }

    conn->device =
        device_create(state->tcpuart_class, NULL, new_dev, NULL, "tcpuart%d", conn->minor);
    if (IS_ERR(conn->device)) {
        pr_err("failed to create device for minor %d\n", conn->minor);
        cdev_del(&conn->cdev);
        sock_release(conn->sock);
        *p_conn = NULL;
        kfree(conn);
        return PTR_ERR(conn->device);
    }

    return 0;
}

ssize_t conn_read(struct connection* conn, size_t count, char __user* dest_buf, int no_block) {
    // First check if there is data left in the conn buffer
    if (conn->read_data_buf_len) {
        pr_info("Sending left ovevr data\n");
        ssize_t send_cnt = min(count, conn->read_data_buf_len);
        if (copy_to_user(dest_buf, conn->read_data_buf, send_cnt)) {
            return -EFAULT;
        }

        memmove(
            conn->read_data_buf, conn->read_data_buf + send_cnt, conn->read_data_buf_len - send_cnt
        );
        conn->read_data_buf_len -= send_cnt;

        return send_cnt;
    }

    if (conn->disconnected) {
        // No more data will be coming in, return 0 to indicate EOF
        return 0;
    }

    // No data in the buffer read from socket until we get a data message
    struct MessageHeader hdr;
    do {
        pr_info("Reading from socket\n");
        int ret = recv_message(&hdr, conn->read_data_buf, conn->sock, no_block);
        if (ret) {
            if (ret == -EAGAIN) {
                return ret;
            } else if (ret == -ECONNRESET) {
                pr_info("Socket was closed by peer\n");
                conn->disconnected = true;
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
        return -EFAULT;
    }

    memmove(
        conn->read_data_buf, conn->read_data_buf + send_cnt, conn->read_data_buf_len - send_cnt
    );
    conn->read_data_buf_len -= send_cnt;

    return send_cnt;
}

int conn_write(struct connection* conn, size_t count, char* buf) {
    if (conn->disconnected) {
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
            conn->disconnected = true;
            return ret;
        }

        return ret;
    }
    return 0;
}

void conn_destroy(struct connection* conn) {
    cdev_del(&conn->cdev);
    device_destroy(conn->tcpuart_class, MKDEV(conn->major, conn->minor));
    sock_release(conn->sock);
    kfree(conn);
}

void conn_open(struct connection* conn) {
    atomic_inc(&conn->open_count);
}

int conn_close(struct connection* conn) {
    if (atomic_dec_and_test(&conn->open_count)) {
        if (conn->disconnected) {
            pr_info("Destroying connection for minor %d\n", conn->minor);
            conn_destroy(conn);
            return CONN_DELETED;
        }
    }
    return 0;
}

int conn_disconnect(struct connection* conn) {
    kernel_sock_shutdown(conn->sock, SHUT_RDWR);
    conn->disconnected = true;

    if (atomic_read(&conn->open_count) == 0) {
        pr_info("Destroying connection for minor %d\n", conn->minor);
        conn_destroy(conn);
        return CONN_DELETED;
    }

    return 0;
}
