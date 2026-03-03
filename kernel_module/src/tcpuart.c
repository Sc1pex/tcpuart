#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include "tcpuart.h"
#include <linux/cdev.h>
#include <linux/fs.h>
#include <linux/init.h>
#include <linux/kernel.h>
#include <linux/module.h>
#include "connection.h"
#include "message.h"
#include "state.h"

static struct tcpuart_state state;

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

        int ret =
            conn_create(&state.conns[conn_idx], conn_idx + 1, conn_cmd.addr, conn_cmd.port, &state);
        if (ret) {
            return ret;
        }

        struct connection* conn = state.conns[conn_idx];
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
        conn_destroy(&state.conns[i], &state);
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
