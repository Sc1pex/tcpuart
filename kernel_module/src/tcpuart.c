#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include "tcpuart.h"
#include <linux/cdev.h>
#include <linux/fs.h>
#include <linux/init.h>
#include <linux/kernel.h>
#include <linux/module.h>

static long handle_ioctl(struct file* file, unsigned int cmd, unsigned long arg) {
    pr_info("Got ioctl: %d\n", cmd);
    return 0;
}

static struct file_operations fops = {
    .owner = THIS_MODULE,
    .unlocked_ioctl = handle_ioctl,
};

static dev_t dev_num;
static struct class* tcpuart_class;
static struct cdev tcpuart_cdev;

static char* tcpuart_devnode(const struct device* dev, umode_t* mode) {
    if (mode) {
        *mode = 0666;
    }
    return NULL;
}

static int __init tcpuart_init(void) {
    alloc_chrdev_region(&dev_num, 0, 1, "tcpuart");
    tcpuart_class = class_create("tcpuart");
    tcpuart_class->devnode = tcpuart_devnode;

    device_create(tcpuart_class, NULL, dev_num, NULL, "tcpuart0");
    cdev_init(&tcpuart_cdev, &fops);
    cdev_add(&tcpuart_cdev, dev_num, 1);

    return 0;
}

static void __exit tcpuart_exit(void) {
    cdev_del(&tcpuart_cdev);
    device_destroy(tcpuart_class, dev_num);
    class_destroy(tcpuart_class);
    unregister_chrdev_region(dev_num, 1);
}

module_init(tcpuart_init);
module_exit(tcpuart_exit);

MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("A serial device working over tcp");
