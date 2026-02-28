#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include "tcpuart.h"
#include <linux/cdev.h>
#include <linux/fs.h>
#include <linux/init.h>
#include <linux/kernel.h>
#include <linux/module.h>

struct tcpuart_state {
    dev_t base_dev_num;
    struct class* tcpuart_class;
    struct cdev ctl_cdev;

    struct file_operations ctl_fops;
};

static struct tcpuart_state state;

static long handle_ctl_ioctl(struct file* file, unsigned int cmd, unsigned long arg) {
    switch (cmd) {
    case TCPUART_CONNECT_TO: {
        struct tcpuart_connect_to conn;
        pr_info("Handling TCPUART_CONNECT_TO ioctl\n");
        pr_info("arg: %lu\n", arg);
        if (copy_from_user(&conn, (void __user*) arg, sizeof(conn))) {
            pr_err("failed to copy data from user\n");
            return -EFAULT;
        }

        pr_info("Received connect request to %pI4:%d\n", &conn.addr, ntohs(conn.port));
        return 0;
    }
    default:
        return -EINVAL;
    }
}

static struct file_operations ctl_fops = {
    .owner = THIS_MODULE,
    .unlocked_ioctl = handle_ctl_ioctl,
};

static char* tcpuart_devnode(const struct device* dev, umode_t* mode) {
    if (mode) {
        *mode = 0666;
    }
    return NULL;
}

static int __init tcpuart_init(void) {
    state.ctl_fops.owner = THIS_MODULE;
    state.ctl_fops.unlocked_ioctl = handle_ctl_ioctl;

    alloc_chrdev_region(&state.base_dev_num, 0, 1, "tcpuart");
    state.tcpuart_class = class_create("tcpuart");
    state.tcpuart_class->devnode = tcpuart_devnode;

    device_create(state.tcpuart_class, NULL, state.base_dev_num, NULL, "tcpuart0");
    cdev_init(&state.ctl_cdev, &ctl_fops);
    cdev_add(&state.ctl_cdev, state.base_dev_num, 1);

    return 0;
}

static void __exit tcpuart_exit(void) {
    cdev_del(&state.ctl_cdev);
    device_destroy(state.tcpuart_class, state.base_dev_num);
    class_destroy(state.tcpuart_class);
    unregister_chrdev_region(state.base_dev_num, 1);
}

module_init(tcpuart_init);
module_exit(tcpuart_exit);

MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("A serial device working over tcp");
