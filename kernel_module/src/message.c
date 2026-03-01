#include "message.h"

static void to_network_order(struct MessageHeader* header) {
    header->kind = htons(header->kind);
    header->size = htons(header->size);
}

static void from_network_order(struct MessageHeader* header) {
    header->kind = ntohs(header->kind);
    header->size = ntohs(header->size);
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

int recv_message(
    struct MessageHeader* header, uint8_t* content, struct socket* socket, int noblock
) {
    struct kvec io;
    io.iov_base = header;
    io.iov_len = sizeof(*header);

    struct msghdr msg = {};
    msg.msg_flags = noblock ? MSG_DONTWAIT : MSG_WAITALL;

    // Read the header
    int res = kernel_recvmsg(socket, &msg, &io, 1, sizeof(*header), msg.msg_flags);
    if (res < 0) {
        return res;
    } else if (res == 0) {
        return -ECONNRESET;
    }

    from_network_order(header);
    res = validate_header(*header);
    if (res) {
        return res;
    }

    // Read the rest of the message
    io.iov_base = content;
    io.iov_len = header->size;
    msg.msg_flags = noblock ? MSG_DONTWAIT : MSG_WAITALL;
    res = kernel_recvmsg(socket, &msg, &io, 1, header->size, msg.msg_flags);
    if (res < 0) {
        return res;
    } else if (res == 0) {
        return -ECONNRESET;
    }

    return 0;
}
