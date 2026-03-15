#include "message.hpp"
#include "utils.hpp"

void sendMessage(WiFiClient& client, MessageHeader header, const uint8_t* message) {
    uint8_t header_buf[4];
    *(uint16_t*) header_buf = htons(header.kind);
    *(uint16_t*) (header_buf + 2) = htons(header.size);

    client.write(header_buf, sizeof(header_buf));
    client.write(message, header.size);
}

ParseMessageResult readMessage(WiFiClient& client, MessageHeader& header, uint8_t* message_buf) {
    if (client.available() < sizeof(MessageHeader)) {
        return ParseMessageResult::NotEnoughData;
    }

    static uint8_t header_buf[4];
    int res = read_all(client, header_buf, sizeof(header_buf));
    if (res < 0) {
        return ParseMessageResult::ReadError;
    }
    header = MessageHeader(header_buf);

    if (header.kind >= MessageKind_Count) {
        return ParseMessageResult::InvalidKind;
    }
    if (header.size > MAX_MESSAGE_SIZE) {
        return ParseMessageResult::InvalidSize;
    }

    read_all(client, message_buf, header.size);
    return ParseMessageResult::Success;
}