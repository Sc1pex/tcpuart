#ifndef _STATE_H
#define _STATE_H

#include <linux/cdev.h>

#define MAX_DEVICES 16
#define MAX_CONNS (MAX_DEVICES - 1)

struct tcpuart_state {
    dev_t base_dev_num;
    struct class* tcpuart_class;

    struct cdev ctl_cdev;
    struct connection* conns[MAX_CONNS];

    struct file_operations ctl_fops;
    struct file_operations conn_fops;
};

#endif
