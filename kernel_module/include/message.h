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

#define MESSAGE_ERROR_INVALID_KIND 1
#define MESSAGE_ERROR_INVALID_SIZE 2

int send_message(struct MessageHeader header, uint8_t* content, struct socket* socket);
int recv_message(struct MessageHeader* header, uint8_t* content, struct socket* socket);

#endif
