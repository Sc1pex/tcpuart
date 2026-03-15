#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include "tcpuart.h"
#include <linux/cdev.h>
#include <linux/init.h>
#include <linux/kernel.h>
#include <linux/module.h>
#include <linux/tty.h>
#include "connection.h"
#include "state.h"

static struct tcpuart_state state;

static int handle_connect_to_ioctl(const struct tcpuart_connect_to* conn_cmd) {
    if (mutex_lock_interruptible(&state.table.mutex)) {
        return -EINTR;
    }
    // Try to find a minor device for connection
    int conn_idx = 0;
    for (; conn_idx < MAX_CONNS; conn_idx++) {
        if (conn_avabile(state.table.conns[conn_idx])) {
            break;
        }
    }
    if (conn_idx == MAX_CONNS) {
        mutex_unlock(&state.table.mutex);
        return -ENOSPC;
    }

    int ret = conn_init(
        state.table.conns[conn_idx], conn_idx + 1, conn_cmd->addr, conn_cmd->port, state.tty_driver
    );
    mutex_unlock(&state.table.mutex);

    if (ret) {
        return ret;
    }

    return conn_idx + 1;
}

static int handle_get_server_info(struct tcpuart_server_info* info) {
    if (info->minor < 1 || info->minor > MAX_CONNS) {
        return -EINVAL;
    }

    if (mutex_lock_interruptible(&state.table.mutex)) {
        return -EINTR;
    }

    struct connection* conn = state.table.conns[info->minor - 1];
    if (conn_avabile(conn)) {
        mutex_unlock(&state.table.mutex);
        return -ENODEV;
    }

    int ret = conn_get_info(conn, info);
    mutex_unlock(&state.table.mutex);

    return ret;
}

static int handle_try_destroy(unsigned int minor) {
    if (minor < 1 || minor > MAX_CONNS) {
        return -EINVAL;
    }

    if (mutex_lock_interruptible(&state.table.mutex)) {
        return -EINTR;
    }

    struct connection* conn = state.table.conns[minor - 1];
    if (conn_avabile(conn)) {
        mutex_unlock(&state.table.mutex);
        return -ENODEV;
    }
    if (conn_in_use(conn)) {
        mutex_unlock(&state.table.mutex);
        return -EBUSY;
    }
    conn_destroy(conn);

    mutex_unlock(&state.table.mutex);

    return 0;
}

static long handle_ctl_ioctl(struct file* file, unsigned int cmd, unsigned long arg) {
    switch (cmd) {
    case TCPUART_CONNECT_TO: {
        struct tcpuart_connect_to conn_cmd;
        if (copy_from_user(&conn_cmd, (void __user*) arg, sizeof(conn_cmd))) {
            return -EFAULT;
        }

        return handle_connect_to_ioctl(&conn_cmd);
    }

    case TCPUART_GET_SERVER_INFO: {
        struct tcpuart_server_info server_info;
        if (copy_from_user(&server_info, (void __user*) arg, sizeof(server_info))) {
            return -EFAULT;
        }

        int ret = handle_get_server_info(&server_info);
        if (ret) {
            return ret;
        }

        if (copy_to_user((void __user*) arg, &server_info, sizeof(server_info))) {
            return -EFAULT;
        }

        return 0;
    }

    case TCPUART_TRY_DESTROY: {
        return handle_try_destroy(arg);
    }

    default:
        return -ENOTTY;
    }
}

static char* tcpuart_devnode(const struct device* dev, umode_t* mode) {
    if (mode) {
        *mode = 0666;
    }
    return NULL;
}

static int __init tcpuart_init(void) {
    mutex_init(&state.table.mutex);

    state.ctl_fops.owner = THIS_MODULE;
    state.ctl_fops.unlocked_ioctl = handle_ctl_ioctl;

    dev_t dev_num;
    int ret = alloc_chrdev_region(&dev_num, 0, 1, "tcpuart");
    if (ret) {
        return ret;
    }

    state.ctl_class = class_create("tcpuart");
    if (IS_ERR(state.ctl_class)) {
        ret = PTR_ERR(state.ctl_class);
        state.ctl_class = NULL;
        unregister_chrdev_region(dev_num, 1);
        return ret;
    }
    state.ctl_class->devnode = tcpuart_devnode;

    cdev_init(&state.ctl_cdev, &state.ctl_fops);
    ret = cdev_add(&state.ctl_cdev, dev_num, 1);
    if (ret) {
        class_destroy(state.ctl_class);
        state.ctl_class = NULL;
        unregister_chrdev_region(dev_num, 1);
        return ret;
    }

    struct device* ctl_dev = device_create(state.ctl_class, NULL, dev_num, NULL, "tcpuart0");
    if (IS_ERR(ctl_dev)) {
        cdev_del(&state.ctl_cdev);
        class_destroy(state.ctl_class);
        state.ctl_class = NULL;
        unregister_chrdev_region(dev_num, 1);
        return PTR_ERR(ctl_dev);
    }

    state.tty_driver = tty_alloc_driver(MAX_DEVICES, TTY_DRIVER_DYNAMIC_DEV | TTY_DRIVER_REAL_RAW);
    if (IS_ERR(state.tty_driver)) {
        cdev_del(&state.ctl_cdev);
        device_destroy(state.ctl_class, dev_num);
        class_destroy(state.ctl_class);
        unregister_chrdev_region(dev_num, 1);
        return PTR_ERR(state.tty_driver);
    }

    state.tty_driver->owner = THIS_MODULE;
    state.tty_driver->driver_name = "tcpuart";
    state.tty_driver->name = "tcpuart";
    state.tty_driver->minor_start = 0;
    state.tty_driver->type = TTY_DRIVER_TYPE_SERIAL;
    state.tty_driver->subtype = SERIAL_TYPE_NORMAL;
    state.tty_driver->init_termios = tty_std_termios;
    state.tty_driver->ops = conn_get_tty_ops();

    for (int i = 0; i < MAX_CONNS; i++) {
        state.table.conns[i] = kzalloc(sizeof(struct connection), GFP_KERNEL);
        if (!state.table.conns[i]) {
            ret = -ENOMEM;
            goto err_free_conns;
        }
    }

    state.tty_driver->driver_state = &state.table;

    ret = tty_register_driver(state.tty_driver);
    if (ret) {
        goto err_free_conns;
    }

    return 0;

err_free_conns:
    for (int i = 0; i < MAX_CONNS; i++) {
        kfree(state.table.conns[i]);
        state.table.conns[i] = NULL;
    }

    tty_driver_kref_put(state.tty_driver);
    state.tty_driver = NULL;
    device_destroy(state.ctl_class, dev_num);
    cdev_del(&state.ctl_cdev);
    class_destroy(state.ctl_class);
    state.ctl_class = NULL;
    unregister_chrdev_region(dev_num, 1);
    return ret;
}

static void __exit tcpuart_exit(void) {
    for (int i = 0; i < MAX_CONNS; i++) {
        if (state.table.conns[i]) {
            conn_destroy(state.table.conns[i]);
            kfree(state.table.conns[i]);
        }
    }

    if (state.tty_driver) {
        tty_unregister_driver(state.tty_driver);
        tty_driver_kref_put(state.tty_driver);
    }

    dev_t dev_num = state.ctl_cdev.dev;
    cdev_del(&state.ctl_cdev);
    device_destroy(state.ctl_class, dev_num);
    class_destroy(state.ctl_class);
    unregister_chrdev_region(dev_num, 1);
}

module_init(tcpuart_init);
module_exit(tcpuart_exit);

MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("A serial device working over tcp");
