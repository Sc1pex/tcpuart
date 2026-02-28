#include "asm-generic/errno-base.h"
#include "linux/err.h"
#include "linux/gfp_types.h"
#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include <linux/cdev.h>
#include <linux/fs.h>
#include <linux/init.h>
#include <linux/kernel.h>
#include <linux/module.h>
#include "tcpuart.h"

#define MAX_DEVICES 16
#define MAX_CONNS (MAX_DEVICES - 1)

struct connection {
    struct cdev cdev;
    struct device* device;
    uint32_t addr;
    uint16_t port;
    int minor;
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

        conn->addr = conn_cmd.addr;
        conn->port = conn_cmd.port;
        // +1 for the ctl device
        conn->minor = conn_idx + 1;

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

        return 0;
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

static int __init tcpuart_init(void) {
    state.ctl_fops.owner = THIS_MODULE;
    state.ctl_fops.unlocked_ioctl = handle_ctl_ioctl;

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
