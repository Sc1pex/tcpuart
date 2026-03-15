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

static void handle_lost_connection(struct connection* conn) {
    if (!test_and_clear_bit(CONN_CONNECTED, &conn->flags)) {
        return;
    }

    if (conn->sock) {
        write_lock_bh(&conn->sock->sk->sk_callback_lock);
        conn->sock->sk->sk_data_ready = conn->old_data_ready;
        conn->sock->sk->sk_user_data = NULL;
        write_unlock_bh(&conn->sock->sk->sk_callback_lock);

        sock_release(conn->sock);
        conn->sock = NULL;
    }

    tty_port_tty_hangup(&conn->port, false);
}

static void conn_rx_work_handler(struct work_struct* work) {
    struct connection* conn = container_of(work, struct connection, rx_work);

    struct MessageHeader hdr;
    uint8_t buf[MAXIMUM_MESSAGE_SIZE];

    while (true) {
        if (test_bit(CONN_THROTTLED, &conn->flags)) {
            break;
        }
        if (!test_bit(CONN_CONNECTED, &conn->flags)) {
            break;
        }

        int ret = recv_message(&hdr, buf, conn->sock);
        if (ret == -EAGAIN || ret == -EINTR) {
            break;
        } else if (ret < 0) {
            handle_lost_connection(conn);
            break;
        }

        if (hdr.kind == MESSAGE_KIND_DATA) {
            size_t ret = tty_insert_flip_string(&conn->port, buf, hdr.size);
            if (ret != hdr.size) {
                pr_err("Partial read. Lost %zu bytes of data", hdr.size - ret);
            }
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

    set_bit(CONN_CONNECTED, &conn->flags);

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

    clear_bit(CONN_CONNECTED, &conn->flags);
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
    conn->flags = 0;

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

void conn_destroy(struct connection* conn) {
    if (!test_bit(CONN_ACTIVE, &conn->flags)) {
        return;
    }

    tty_port_unregister_device(&conn->port, conn->driver, conn->minor);
    tty_port_destroy(&conn->port);
    clear_bit(CONN_ACTIVE, &conn->flags);
    clear_bit(CONN_CONNECTED, &conn->flags);
}

int conn_get_info(struct connection* conn, struct tcpuart_server_info* info) {
    if (!test_bit(CONN_ACTIVE, &conn->flags)) {
        return -ENOTCONN;
    }

    info->addr = conn->sock_addr;
    info->port = conn->sock_port;

    return 0;
}

static int conn_open(struct tty_struct* tty, struct file* file) {
    int minor = tty->index;
    if (minor < 1 || minor > MAX_CONNS) {
        return -ENODEV;
    }
    struct conn_table* table = tty->driver->driver_state;

    tty->driver_data = table->conns[minor - 1];
    return tty_port_open(&table->conns[minor - 1]->port, tty, file);
}

static void conn_close(struct tty_struct* tty, struct file* file) {
    struct connection* conn = tty->driver_data;
    tty_port_close(&conn->port, tty, file);
}

static ssize_t conn_write(struct tty_struct* tty, const unsigned char* buf, size_t count) {
    struct connection* conn = tty->driver_data;

    if (!test_bit(CONN_CONNECTED, &conn->flags)) {
        return -EIO;
    }

    ssize_t written_cnt = 0;

    while (count) {
        size_t copy_cnt = min(count, MAXIMUM_MESSAGE_SIZE);

        struct MessageHeader hdr = {
            .kind = MESSAGE_KIND_DATA,
            .size = copy_cnt,
        };
        int ret = send_message(hdr, buf, conn->sock);
        if (ret < 0) {
            if (ret != -EAGAIN && ret != -EINTR) {
                handle_lost_connection(conn);
            }
            return ret;
        }

        count -= copy_cnt;
        written_cnt += copy_cnt;
        buf += copy_cnt;
    }

    return written_cnt;
}

static unsigned int conn_write_room(struct tty_struct* tty) {
    struct connection* conn = tty->driver_data;
    if (!test_bit(CONN_CONNECTED, &conn->flags)) {
        return 0;
    }
    return MAXIMUM_MESSAGE_SIZE;
}

static void conn_throttle(struct tty_struct* tty) {
    struct connection* conn = tty->driver_data;
    set_bit(CONN_THROTTLED, &conn->flags);
}

static void conn_unthrottle(struct tty_struct* tty) {
    struct connection* conn = tty->driver_data;
    clear_bit(CONN_THROTTLED, &conn->flags);
    schedule_work(&conn->rx_work);
}

static void conn_set_termios(struct tty_struct* tty, const struct ktermios* old) {
    struct connection* conn = tty->driver_data;

    if (!test_bit(CONN_CONNECTED, &conn->flags)) {
        if (old) {
            tty->termios = *old;
        }
        return;
    }
    if (old && !tty_termios_hw_change(old, &tty->termios)) {
        return;
    }

    struct MessageConfigData cfg = {};
    cfg.baud = htonl(tty_get_baud_rate(tty));

    switch (tty->termios.c_cflag & CSIZE) {
    case CS5:
        cfg.data_bits = 5;
        break;
    case CS6:
        cfg.data_bits = 6;
        break;
    case CS7:
        cfg.data_bits = 7;
        break;
    case CS8:
        cfg.data_bits = 8;
        break;
    default:
        pr_err("Invalid data bits\n");
        return;
    }

    cfg.stop_bits = (tty->termios.c_cflag & CSTOPB) ? 2 : 1;

    if (tty->termios.c_cflag & PARENB) {
        if (tty->termios.c_cflag & PARODD) {
            cfg.parity = MESSAGE_CONFIG_PARITY_ODD;
        } else {
            cfg.parity = MESSAGE_CONFIG_PARITY_EVEN;
        }
    } else {
        cfg.parity = MESSAGE_CONFIG_PARITY_NONE;
    }

    struct MessageHeader hdr = {
        .kind = MESSAGE_KIND_CONFIG,
        .size = sizeof(cfg),
    };
    int ret = send_message(hdr, (const uint8_t*) &cfg, conn->sock);
    if (ret < 0) {
        if (old) {
            tty->termios = *old;
        }
        handle_lost_connection(conn);
    }
}

static const struct tty_operations conn_ops = {
    .open = conn_open,
    .close = conn_close,
    .write = conn_write,
    .write_room = conn_write_room,
    .throttle = conn_throttle,
    .unthrottle = conn_unthrottle,
    .set_termios = conn_set_termios,
};

const struct tty_operations* conn_get_tty_ops(void) {
    return &conn_ops;
}
