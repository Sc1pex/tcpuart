#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include "connection.h"
#include <linux/tty_flip.h>
#include "message.h"
#include "net/sock.h"

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

static void conn_rx_work_handler(struct work_struct* work) {
    struct connection* conn = container_of(work, struct connection, rx_work);

    struct MessageHeader hdr;
    uint8_t buf[MAXIMUM_MESSAGE_SIZE];

    while (true) {
        int ret = recv_message(&hdr, buf, conn->sock);
        if (ret == -EAGAIN) {
            break;
        }
        if (ret < 0) {
            pr_info("socket error in rx_work: %d\n", ret);
            break;
        }

        if (hdr.kind == MESSAGE_KIND_DATA) {
            tty_insert_flip_string(&conn->port, buf, hdr.size);
        }
    }

    tty_flip_buffer_push(&conn->port);
}

static void conn_data_ready(struct sock* sk) {
    struct connection* conn = sk->sk_user_data;
    schedule_work(&conn->rx_work);
}

static int conn_activate(struct tty_port* port, struct tty_struct* tty) {
    struct connection* conn = container_of(port, struct connection, port);

    int ret = create_tcp_socket(&conn->sock, conn->sock_addr, conn->sock_port);
    if (ret) {
        pr_err("Failed to connect to server\n");
        return ret;
    }

    write_lock_bh(&conn->sock->sk->sk_callback_lock);
    conn->old_data_ready = conn->sock->sk->sk_data_ready;
    conn->sock->sk->sk_user_data = conn;
    conn->sock->sk->sk_data_ready = conn_data_ready;
    write_unlock_bh(&conn->sock->sk->sk_callback_lock);

    return 0;
}

static void conn_shutdown(struct tty_port* port) {
    struct connection* conn = container_of(port, struct connection, port);

    if (!conn->sock) {
        return;
    }

    write_lock_bh(&conn->sock->sk->sk_callback_lock);
    conn->sock->sk->sk_data_ready = conn->old_data_ready;
    conn->sock->sk->sk_user_data = NULL;
    write_unlock_bh(&conn->sock->sk->sk_callback_lock);

    cancel_work_sync(&conn->rx_work);
    kernel_sock_shutdown(conn->sock, SHUT_RDWR);
    sock_release(conn->sock);
    conn->sock = NULL;
}

static const struct tty_port_operations port_ops = {
    .activate = conn_activate,
    .shutdown = conn_shutdown,
};

int conn_init(
    struct connection* conn, int minor, uint32_t addr, uint16_t port, struct tty_driver* driver
) {
    conn->minor = minor;
    conn->sock = NULL;
    conn->old_data_ready = NULL;
    conn->sock_addr = addr;
    conn->sock_port = port;
    conn->driver = driver;

    tty_port_init(&conn->port);
    conn->port.ops = &port_ops;
    struct device* dev = tty_port_register_device(&conn->port, driver, minor, NULL);
    if (IS_ERR(dev)) {
        tty_port_destroy(&conn->port);
        return PTR_ERR(dev);
    }

    set_bit(CONN_ACTIVE, &conn->flags);
    INIT_WORK(&conn->rx_work, conn_rx_work_handler);

    return 0;
}

int conn_avabile(struct connection* conn) {
    return !test_bit(CONN_ACTIVE, &conn->flags);
}

int conn_write(struct connection* conn, const unsigned char* buf, size_t count) {
    ssize_t written_cnt = 0;

    while (count) {
        size_t copy_cnt = min(count, MAXIMUM_MESSAGE_SIZE);

        struct MessageHeader hdr = {
            .kind = MESSAGE_KIND_DATA,
            .size = copy_cnt,
        };
        int ret = send_message(hdr, buf, conn->sock);
        if (ret) {
            return ret;
        }

        count -= copy_cnt;
        written_cnt += copy_cnt;
        buf += copy_cnt;
    }

    return written_cnt;
}

void conn_destroy(struct connection* conn) {
    if (!test_bit(CONN_ACTIVE, &conn->flags)) {
        return;
    }

    tty_port_unregister_device(&conn->port, conn->driver, conn->minor);
    tty_port_destroy(&conn->port);
    clear_bit(CONN_ACTIVE, &conn->flags);
}

int conn_get_info(struct connection* conn, struct tcpuart_server_info* info) {
    if (!test_bit(CONN_ACTIVE, &conn->flags)) {
        return -ENOTCONN;
    }

    info->addr = conn->sock_addr;
    info->port = conn->sock_port;

    return 0;
}
