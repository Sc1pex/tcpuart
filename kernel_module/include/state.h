#ifndef _STATE_H
#define _STATE_H

#include <linux/cdev.h>
#include <linux/mutex.h>
#include <linux/tty_driver.h>

#define MAX_DEVICES 16
#define MAX_CONNS (MAX_DEVICES - 1)

struct conn_table {
    struct connection* conns[MAX_CONNS];
    struct mutex mutex;
};

struct tcpuart_state {
    struct class* ctl_class;
    struct cdev ctl_cdev;
    struct file_operations ctl_fops;

    struct conn_table table;

    struct tty_driver* tty_driver;
};

#endif
