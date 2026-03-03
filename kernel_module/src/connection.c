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

    conn->minor = minor;

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

void conn_destroy(struct connection** conn, const struct tcpuart_state* state) {
    if (*conn) {
        cdev_del(&(*conn)->cdev);
        device_destroy(state->tcpuart_class, MKDEV(MAJOR(state->base_dev_num), (*conn)->minor));
        sock_release((*conn)->sock);
        kfree(*conn);
        *conn = NULL;
    }
}
