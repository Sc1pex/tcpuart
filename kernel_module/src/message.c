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
        return -EPROTO;
    }
    if (header.size > MAXIMUM_MESSAGE_SIZE) {
        return -EPROTO;
    }
    return 0;
}

int send_message(struct MessageHeader header, const uint8_t* content, struct socket* socket) {
    int res = validate_header(header);
    if (res) {
        return res;
    }

    struct kvec io[2];

    io[0].iov_base = &header;
    io[0].iov_len = sizeof(header);

    io[1].iov_base = (void*) content;
    io[1].iov_len = header.size;

    struct msghdr msg = {};
    size_t msg_len = sizeof(header) + header.size;

    to_network_order(&header);
    res = kernel_sendmsg(socket, &msg, io, 2, msg_len);

    if (res != msg_len) {
        return (res < 0) ? res : -EIO;
    }
    return 0;
}

int recv_message(struct MessageHeader* header, uint8_t* content, struct socket* socket) {
    struct msghdr msg = {};
    struct kvec io[2];

    // First peek the header and data to ensure the entire message is available
    io[0].iov_base = header;
    io[0].iov_len = sizeof(*header);
    int res = kernel_recvmsg(socket, &msg, io, 1, sizeof(*header), MSG_DONTWAIT | MSG_PEEK);
    if (res < 0) {
        return res;
    }
    if (res == 0) {
        return -ECONNRESET;
    }
    if (res != sizeof(*header)) {
        return -EAGAIN; // Not enough data for the header yet
    }

    from_network_order(header);
    res = validate_header(*header);
    if (res) {
        return res;
    }

    // Peek the content
    io[1].iov_base = content;
    io[1].iov_len = header->size;
    size_t total_len = sizeof(*header) + header->size;
    res = kernel_recvmsg(socket, &msg, io, 2, total_len, MSG_DONTWAIT | MSG_PEEK);
    if (res < 0) {
        return res;
    }
    if (res == 0) {
        return -ECONNRESET;
    }
    if (res != total_len) {
        return -EAGAIN; // Not enough data for the content yet
    }

    // Consume the entire message
    res = kernel_recvmsg(socket, &msg, io, 2, total_len, MSG_DONTWAIT);
    if (res != total_len) {
        return (res < 0) ? res : -EIO;
    }

    return 0;
}
