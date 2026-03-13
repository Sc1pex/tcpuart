#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include "tcpuart.h"
#include <linux/cdev.h>
#include <linux/init.h>
#include <linux/kernel.h>
#include <linux/module.h>
#include <linux/tty.h>
#include "connection.h"
#include "message.h"
#include "state.h"

static struct tcpuart_state state;

static int handle_connect_to_ioctl(const struct tcpuart_connect_to* conn_cmd) {
    if (mutex_lock_interruptible(&state.mutex)) {
        return -EINTR;
    }
    // Try to find a minor device for connection
    int conn_idx = 0;
    for (; conn_idx < MAX_CONNS; conn_idx++) {
        if (conn_avabile(state.conns[conn_idx])) {
            break;
        }
    }
    if (conn_idx == MAX_CONNS) {
        pr_err("no free connection slot\n");
        mutex_unlock(&state.mutex);
        return -ENOSPC;
    }

    int ret = conn_init(
        state.conns[conn_idx], conn_idx + 1, conn_cmd->addr, conn_cmd->port, state.tty_driver
    );
    mutex_unlock(&state.mutex);

    if (ret) {
        return ret;
    }

    pr_info(
        "created /dev/tcpuart%d for %pI4:%d\n", conn_idx + 1, &conn_cmd->addr, ntohs(conn_cmd->port)
    );
    return conn_idx + 1;
}

static int handle_get_server_info(struct tcpuart_server_info* info) {
    if (info->minor < 1 || info->minor > MAX_CONNS) {
        pr_err("invalid minor number: %d\n", info->minor);
        return -EINVAL;
    }

    if (mutex_lock_interruptible(&state.mutex)) {
        return -EINTR;
    }

    struct connection* conn = state.conns[info->minor - 1];
    if (conn_avabile(conn)) {
        mutex_unlock(&state.mutex);
        return -ENODEV;
    }

    int ret = conn_get_info(conn, info);
    mutex_unlock(&state.mutex);

    return ret;
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

    case TCPUART_GET_SERVER_INFO: {
        struct tcpuart_server_info server_info;
        if (copy_from_user(&server_info, (void __user*) arg, sizeof(server_info))) {
            pr_err("failed to copy data from user\n");
            return -EFAULT;
        }

        int ret = handle_get_server_info(&server_info);
        if (ret) {
            return ret;
        }

        if (copy_to_user((void __user*) arg, &server_info, sizeof(server_info))) {
            pr_err("failed to copy data to user\n");
            return -EFAULT;
        }

        return 0;
    }

    default:
        pr_info("Invalid ioctl number\n");
        return -ENOTTY;
    }
}

static char* tcpuart_devnode(const struct device* dev, umode_t* mode) {
    if (mode) {
        *mode = 0666;
    }
    return NULL;
}

static ssize_t handle_conn_write(struct tty_struct* tty, const unsigned char* buf, size_t count) {
    struct connection* conn = tty->driver_data;
    return conn_write(conn, buf, count);
}

static int handle_conn_open(struct tty_struct* tty, struct file* file) {
    int minor = tty->index;
    if (minor < 1 || minor > MAX_CONNS) {
        return -ENODEV;
    }

    tty->driver_data = state.conns[minor - 1];
    return tty_port_open(&state.conns[minor - 1]->port, tty, file);
}

static void handle_conn_close(struct tty_struct* tty, struct file* file) {
    struct connection* conn = tty->driver_data;
    tty_port_close(&conn->port, tty, file);
}

static unsigned int handle_conn_write_room(struct tty_struct* tty) {
    struct connection* conn = tty->driver_data;
    if (!conn->sock) {
        return 0;
    }
    return MAXIMUM_MESSAGE_SIZE;
}

static const struct tty_operations conn_ops = {
    .open = handle_conn_open,
    .close = handle_conn_close,
    .write = handle_conn_write,
    .write_room = handle_conn_write_room,
};

static int __init tcpuart_init(void) {
    mutex_init(&state.mutex);

    state.ctl_fops.owner = THIS_MODULE;
    state.ctl_fops.unlocked_ioctl = handle_ctl_ioctl;

    dev_t dev_num;
    alloc_chrdev_region(&dev_num, 0, 1, "tcpuart");
    state.ctl_class = class_create("tcpuart");
    state.ctl_class->devnode = tcpuart_devnode;

    cdev_init(&state.ctl_cdev, &state.ctl_fops);
    cdev_add(&state.ctl_cdev, dev_num, 1);
    device_create(state.ctl_class, NULL, dev_num, NULL, "tcpuart0");

    state.tty_driver = tty_alloc_driver(MAX_DEVICES, TTY_DRIVER_DYNAMIC_DEV | TTY_DRIVER_REAL_RAW);
    if (IS_ERR(state.tty_driver)) {
        cdev_del(&state.ctl_cdev);
        device_destroy(state.ctl_class, dev_num);
        class_destroy(state.ctl_class);
        unregister_chrdev_region(dev_num, MAX_DEVICES);
        return PTR_ERR(state.tty_driver);
    }

    state.tty_driver->owner = THIS_MODULE;
    state.tty_driver->driver_name = "tcpuart";
    state.tty_driver->name = "tcpuart";
    state.tty_driver->minor_start = 0;
    state.tty_driver->type = TTY_DRIVER_TYPE_SERIAL;
    state.tty_driver->subtype = SERIAL_TYPE_NORMAL;
    state.tty_driver->init_termios = tty_std_termios;
    state.tty_driver->ops = &conn_ops;

    tty_register_driver(state.tty_driver);

    for (int i = 0; i < MAX_CONNS; i++) {
        state.conns[i] = kzalloc(sizeof(struct connection), GFP_KERNEL);
    }

    return 0;
}

static void __exit tcpuart_exit(void) {
    for (int i = 0; i < MAX_CONNS; i++) {
        if (state.conns[i]) {
            conn_destroy(state.conns[i]);
            kfree(state.conns[i]);
        }
    }

    tty_unregister_driver(state.tty_driver);
    tty_driver_kref_put(state.tty_driver);

    dev_t dev_num = state.ctl_cdev.dev;
    cdev_del(&state.ctl_cdev);
    device_destroy(state.ctl_class, dev_num);
    class_destroy(state.ctl_class);
    unregister_chrdev_region(dev_num, MAX_DEVICES);
}

module_init(tcpuart_init);
module_exit(tcpuart_exit);

MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("A serial device working over tcp");
