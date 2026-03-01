#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include "tcpuart.h"
#include <linux/cdev.h>
#include <linux/fs.h>
#include <linux/init.h>
#include <linux/kernel.h>
#include <linux/module.h>
#include <net/sock.h>
#include "message.h"

#define MAX_DEVICES 16
#define MAX_CONNS (MAX_DEVICES - 1)

struct connection {
    struct cdev cdev;
    struct device* device;
    int minor;

    struct socket* sock;

    uint8_t read_data_buf[MAXIMUM_MESSAGE_SIZE];
    size_t read_data_buf_len;
};

struct tcpuart_state {
    dev_t base_dev_num;
    struct class* tcpuart_class;

    struct cdev ctl_cdev;
    struct connection* conns[MAX_CONNS];

    struct file_operations ctl_fops;
    struct file_operations conn_fops;
};

static struct tcpuart_state state;

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
        sock = NULL;
        return rc;
    }

    return 0;
}

static long handle_ctl_ioctl(struct file* file, unsigned int cmd, unsigned long arg) {
    switch (cmd) {
    case TCPUART_CONNECT_TO: {
        struct tcpuart_connect_to conn_cmd;
        if (copy_from_user(&conn_cmd, (void __user*) arg, sizeof(conn_cmd))) {
            pr_err("failed to copy data from user\n");
            return -EFAULT;
        }

        // Try to find a minor device for connection
        int conn_idx = 0;
        for (; conn_idx < MAX_CONNS; conn_idx++) {
            if (!state.conns[conn_idx]) {
                break;
            }
        }
        if (conn_idx == MAX_CONNS) {
            pr_err("no free connection slot\n");
            return -ENOSPC;
        }

        struct connection* conn = kzalloc(sizeof(*conn), GFP_KERNEL);
        if (!conn) {
            return -ENOMEM;
        }

        // +1 for the ctl device
        conn->minor = conn_idx + 1;

        // Try to connect to the socket
        int rc = create_tcp_socket(&conn->sock, conn_cmd.addr, conn_cmd.port);
        if (rc) {
            pr_err("failed to connect to tcp server\n");
            kfree(conn);
            return rc;
        }

        dev_t new_dev = MKDEV(MAJOR(state.base_dev_num), conn->minor);
        cdev_init(&conn->cdev, &state.conn_fops);
        if (cdev_add(&conn->cdev, new_dev, 1)) {
            pr_err("failed to add cdev for conn\n");
            kfree(conn);
            return -ENOMEM;
        }

        conn->device =
            device_create(state.tcpuart_class, NULL, new_dev, NULL, "tcpuart%d", conn->minor);
        if (IS_ERR(conn->device)) {
            pr_err("failed to create device for minor %d\n", conn->minor);
            cdev_del(&conn->cdev);
            kfree(conn);
            return PTR_ERR(conn->device);
        }

        state.conns[conn_idx] = conn;
        pr_info(
            "created /dev/tcpuart%d for %pI4:%d\n", conn->minor, &conn_cmd.addr,
            ntohs(conn_cmd.port)
        );

        return conn->minor;
    }

    default:
        return -EINVAL;
    }
}

static char* tcpuart_devnode(const struct device* dev, umode_t* mode) {
    if (mode) {
        *mode = 0666;
    }
    return NULL;
}

static ssize_t conn_read(struct file* file, char __user* buf, size_t count, loff_t* ppos) {
    struct connection* conn = file->private_data;
    if (!conn) {
        return -ENODEV;
    }

    // First check if there is data left in the conn buffer
    if (conn->read_data_buf_len) {
        pr_info("Sending left ovevr data\n");
        size_t send_cnt = min(count, conn->read_data_buf_len);
        if (copy_to_user(buf, conn->read_data_buf, send_cnt)) {
            return -EFAULT;
        }

        memmove(
            conn->read_data_buf, conn->read_data_buf + send_cnt, conn->read_data_buf_len - send_cnt
        );
        conn->read_data_buf_len -= send_cnt;

        return send_cnt;
    }

    int noblock = file->f_flags & O_NONBLOCK;

    // No data in the buffer read from socket until we get a data message
    struct MessageHeader hdr;
    do {
        pr_info("Reading from socket\n");
        int ret = recv_message(&hdr, conn->read_data_buf, conn->sock, noblock);
        if (ret) {
            if (ret == -EAGAIN) {
                return -EAGAIN;
            }

            pr_err("Failed to receive message: %d\n", ret);
            return ret < 0 ? ret : -EFAULT;
        }
        conn->read_data_buf_len = hdr.size;
    } while (hdr.kind != MESSAGE_KIND_DATA);

    pr_info("Received data message of size: %zu\n", conn->read_data_buf_len);
    size_t send_cnt = min(count, conn->read_data_buf_len);
    if (copy_to_user(buf, conn->read_data_buf, send_cnt)) {
        return -EFAULT;
    }

    memmove(
        conn->read_data_buf, conn->read_data_buf + send_cnt, conn->read_data_buf_len - send_cnt
    );
    conn->read_data_buf_len -= send_cnt;

    return send_cnt;
}

static ssize_t conn_write(struct file* file, const char __user* buf, size_t count, loff_t* ppos) {
    struct connection* conn = file->private_data;
    if (!conn) {
        return -ENODEV;
    }

    char kbuf[MAXIMUM_MESSAGE_SIZE];
    size_t written_cnt = 0;

    while (count) {
        size_t copy_cnt = min(count, MAXIMUM_MESSAGE_SIZE);
        count -= copy_cnt;
        written_cnt += copy_cnt;

        if (copy_from_user(kbuf, buf, copy_cnt)) {
            return -EFAULT;
        }

        struct MessageHeader hdr = {
            .kind = MESSAGE_KIND_DATA,
            .size = copy_cnt,
        };

        int ret = send_message(hdr, kbuf, conn->sock);
        if (ret) {
            pr_err("Failed to send message: %d\n", ret);
            return ret < 0 ? ret : -EFAULT;
        }
    }

    return written_cnt;
}

static int conn_open(struct inode* inode, struct file* file) {
    int minor = iminor(inode);
    struct connection* conn = state.conns[minor - 1];
    if (!conn) {
        return -ENODEV;
    }

    pr_info("Opened connection %d\n", minor);
    file->private_data = conn;
    return 0;
}

static int conn_release(struct inode* inode, struct file* file) {
    file->private_data = NULL;
    return 0;
}

static int __init tcpuart_init(void) {
    state.ctl_fops.owner = THIS_MODULE;
    state.ctl_fops.unlocked_ioctl = handle_ctl_ioctl;

    state.conn_fops.owner = THIS_MODULE;
    state.conn_fops.write = conn_write;
    state.conn_fops.read = conn_read;
    state.conn_fops.open = conn_open;
    state.conn_fops.release = conn_release;

    alloc_chrdev_region(&state.base_dev_num, 0, MAX_DEVICES, "tcpuart");
    state.tcpuart_class = class_create("tcpuart");
    state.tcpuart_class->devnode = tcpuart_devnode;

    cdev_init(&state.ctl_cdev, &state.ctl_fops);
    cdev_add(&state.ctl_cdev, state.base_dev_num, 1);
    device_create(state.tcpuart_class, NULL, state.base_dev_num, NULL, "tcpuart0");

    return 0;
}

static void __exit tcpuart_exit(void) {
    for (int i = 0; i < MAX_CONNS; i++) {
        if (state.conns[i]) {
            cdev_del(&state.conns[i]->cdev);
            device_destroy(
                state.tcpuart_class, MKDEV(MAJOR(state.base_dev_num), state.conns[i]->minor)
            );
            kfree(state.conns[i]);
            state.conns[i] = NULL;
        }
    }

    cdev_del(&state.ctl_cdev);
    device_destroy(state.tcpuart_class, state.base_dev_num);
    class_destroy(state.tcpuart_class);
    unregister_chrdev_region(state.base_dev_num, MAX_DEVICES);
}

module_init(tcpuart_init);
module_exit(tcpuart_exit);

MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("A serial device working over tcp");
