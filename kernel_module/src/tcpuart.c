#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include "tcpuart.h"
#include <linux/cdev.h>
#include <linux/init.h>
#include <linux/kernel.h>
#include <linux/module.h>
#include "connection.h"
#include "message.h"
#include "state.h"

static struct tcpuart_state state;

static int handle_connect_to_ioctl(const struct tcpuart_connect_to* conn_cmd) {
    // Try to find a minor device for connection
    if (mutex_lock_interruptible(&state.mutex)) {
        return -EINTR;
    }
    int conn_idx = 0;
    for (; conn_idx < MAX_CONNS; conn_idx++) {
        if (!state.conns[conn_idx]) {
            break;
        }
    }
    if (conn_idx == MAX_CONNS) {
        pr_err("no free connection slot\n");
        mutex_unlock(&state.mutex);
        return -ENOSPC;
    }

    int ret =
        conn_create(&state.conns[conn_idx], conn_idx + 1, conn_cmd->addr, conn_cmd->port, &state);
    mutex_unlock(&state.mutex);

    if (ret) {
        return ret;
    }

    struct connection* conn = state.conns[conn_idx];
    pr_info(
        "created /dev/tcpuart%d for %pI4:%d\n", conn->minor, &conn_cmd->addr, ntohs(conn_cmd->port)
    );

    return conn->minor;
}

static int handle_disconnect_ioctl(unsigned int minor) {
    if (minor < 1 || minor > MAX_CONNS) {
        pr_err("invalid minor number: %d\n", minor);
        return -EINVAL;
    }

    int conn_idx = minor - 1;
    if (!state.conns[conn_idx]) {
        pr_err("no connection for minor number: %d\n", minor);
        return -ENODEV;
    }

    if (conn_disconnect(state.conns[conn_idx]) == CONN_DELETED) {
        state.conns[conn_idx] = NULL;
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

        return handle_connect_to_ioctl(&conn_cmd);
    }
    case TCPUART_DISCONNECT: {
        pr_info("Got disconnect ioctl with arg: %lu\n", arg);
        return handle_disconnect_ioctl(arg);
    }

    default:
        pr_info("Invalid ioctl number\n");
        return -EINVAL;
    }
}

static char* tcpuart_devnode(const struct device* dev, umode_t* mode) {
    if (mode) {
        *mode = 0666;
    }
    return NULL;
}

static ssize_t handle_conn_read(struct file* file, char __user* buf, size_t count, loff_t* ppos) {
    struct connection* conn = file->private_data;
    if (!conn) {
        return -ENODEV;
    }
    int noblock = file->f_flags & O_NONBLOCK;

    return conn_read(conn, count, buf, noblock);
}

static ssize_t
    handle_conn_write(struct file* file, const char __user* buf, size_t count, loff_t* ppos) {
    struct connection* conn = file->private_data;
    if (!conn) {
        return -ENODEV;
    }

    char kbuf[MAXIMUM_MESSAGE_SIZE];
    ssize_t written_cnt = 0;

    while (count) {
        size_t copy_cnt = min(count, MAXIMUM_MESSAGE_SIZE);
        if (copy_from_user(kbuf, buf, copy_cnt)) {
            return -EFAULT;
        }

        count -= copy_cnt;
        written_cnt += copy_cnt;
        buf += copy_cnt;

        int ret = conn_write(conn, copy_cnt, kbuf);
        if (ret) {
            return ret;
        }
    }

    return written_cnt;
}

static int handle_conn_open(struct inode* inode, struct file* file) {
    int minor = iminor(inode);

    if (mutex_lock_interruptible(&state.mutex)) {
        return -EINTR;
    }
    struct connection* conn = state.conns[minor - 1];
    mutex_unlock(&state.mutex);

    if (!conn) {
        return -ENODEV;
    }

    conn_open(conn);
    file->private_data = conn;
    return 0;
}

static int handle_conn_release(struct inode* inode, struct file* file) {
    int minor = iminor(inode);

    if (mutex_lock_interruptible(&state.mutex)) {
        return -EINTR;
    }

    if (!state.conns[minor - 1]) {
        mutex_unlock(&state.mutex);
        return -ENODEV;
    }

    if (conn_close(state.conns[minor - 1]) == CONN_DELETED) {
        state.conns[minor - 1] = NULL;
    }

    mutex_unlock(&state.mutex);

    file->private_data = NULL;
    return 0;
}

static int __init tcpuart_init(void) {
    mutex_init(&state.mutex);

    state.ctl_fops.owner = THIS_MODULE;
    state.ctl_fops.unlocked_ioctl = handle_ctl_ioctl;

    state.conn_fops.owner = THIS_MODULE;
    state.conn_fops.write = handle_conn_write;
    state.conn_fops.read = handle_conn_read;
    state.conn_fops.open = handle_conn_open;
    state.conn_fops.release = handle_conn_release;

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
            conn_destroy(state.conns[i]);
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
