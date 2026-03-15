#ifndef _MESSAGE_H
#define _MESSAGE_H

#include <linux/types.h>
#include <net/sock.h>

#define MAXIMUM_MESSAGE_SIZE 1024

enum MessageKind {
    MESSAGE_KIND_DATA,
    MESSAGE_KIND_CONFIG,
    MESSAGE_KIND__COUNT,
};

struct MessageHeader {
    uint16_t kind;
    uint16_t size;
};

enum MessageConfigParity {
    MESSAGE_CONFIG_PARITY_NONE,
    MESSAGE_CONFIG_PARITY_EVEN,
    MESSAGE_CONFIG_PARITY_ODD,
};

struct MessageConfigData {
    uint32_t baud;
    uint8_t data_bits;
    uint8_t stop_bits;
    uint8_t parity;
};

int send_message(struct MessageHeader header, const uint8_t* content, struct socket* socket);
int recv_message(struct MessageHeader* header, uint8_t* content, struct socket* socket);

#endif
