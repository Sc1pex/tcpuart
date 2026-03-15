#pragma once

#include <Arduino.h>
#include <ESP8266WiFi.h>
#include <cstdint>

#define MAX_MESSAGE_SIZE 1024

enum MessageKind {
    MessageKindData,
    MessageKindConfig,
    MessageKind_Count,
};

struct MessageHeader {
    uint16_t kind;
    uint16_t size;

    MessageHeader(uint16_t kind, uint16_t size) : kind(kind), size(size) {
    }

    MessageHeader(uint8_t* buffer) {
        kind = ntohs(*(uint16_t*) buffer);
        size = ntohs(*(uint16_t*) (buffer + 2));
    }
};

struct MessageConfigData {
    uint32_t baud;
    uint8_t data_bits;
    uint8_t stop_bits;
    uint8_t parity;
};

enum class ParseMessageResult {
    Success,
    NotEnoughData,
    InvalidKind,
    InvalidSize,
};

void sendMessage(WiFiClient& client, MessageHeader header, const uint8_t* message);

ParseMessageResult readMessage(WiFiClient& client, MessageHeader& header, uint8_t* message_buf);