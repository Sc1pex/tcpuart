#include "message.h"
#include "linux/net.h"

static void to_network_order(struct MessageHeader* header) {
    header->kind = htons(header->kind);
    header->size = htons(header->size);
}

static int validate_header(struct MessageHeader header) {
    if (header.kind >= MESSAGE_KIND__COUNT) {
        return MESSAGE_ERROR_INVALID_KIND;
    }
    if (header.size > MAXIMUM_MESSAGE_SIZE) {
        return MESSAGE_ERROR_INVALID_SIZE;
    }
    return 0;
}

int send_message(struct MessageHeader header, uint8_t* content, struct socket* socket) {
    int res = validate_header(header);
    if (res) {
        return res;
    }

    struct kvec io[2];

    io[0].iov_base = &header;
    io[0].iov_len = sizeof(header);

    io[1].iov_base = content;
    io[1].iov_len = header.size;

    struct msghdr msg = {};
    size_t msg_len = sizeof(header) + header.size;

    to_network_order(&header);
    res = kernel_sendmsg(socket, &msg, io, 2, msg_len);
    if (res < 0) {
        pr_err("Failed to send packet\n");
        return res;
    }

    return 0;
}

int recv_message(struct MessageHeader* header, uint8_t* content, struct socket* socket) {
    return 0;
}
