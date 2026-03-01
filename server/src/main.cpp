#include <Arduino.h>
#include <ESP8266WiFi.h>
#include <WiFiClient.h>

#define MAX_MESSAGE_SIZE 1024

WiFiServer server(15113);

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

    void debugPrint() {
        Serial.print("MessageHeader { kind: ");
        Serial.print(kind);
        Serial.print(", size: ");
        Serial.print(size);
        Serial.println(" }");
    }
};

enum class ParseMessageResult {
    Success,
    NotEnoughData,
    InvalidKind,
    InvalidSize,
};

void read_all(WiFiClient& client, uint8_t* buffer, size_t size) {
    size_t total_read = 0;
    while (total_read < size) {
        if (client.available()) {
            int bytes_read = client.read(buffer + total_read, size - total_read);
            if (bytes_read > 0) {
                total_read += bytes_read;
            }
        }
    }
}

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
    read_all(client, header_buf, sizeof(header_buf));
    header = MessageHeader(header_buf);

    if (header.kind >= MessageKind_Count) {
        Serial.print("Invalid message kind: ");
        Serial.println(header.kind);
        return ParseMessageResult::InvalidKind;
    }
    if (header.size > MAX_MESSAGE_SIZE) {
        Serial.print("Invalid message size: ");
        Serial.println(header.size);
        return ParseMessageResult::InvalidSize;
    }

    read_all(client, message_buf, header.size);
    return ParseMessageResult::Success;
}

void setup() {
    Serial.begin(115200);
    WiFi.begin(WIFI_SSID, WIFI_PASSWORD);

    while (WiFi.status() != WL_CONNECTED) {
        delay(500);
    }
    Serial.println("Connected to WiFi");
    Serial.print("IP address: ");
    Serial.println(WiFi.localIP());

    server.begin();
    server.setNoDelay(true);
}

uint8_t message_buf[MAX_MESSAGE_SIZE];
uint8_t serial_buf[128];

WiFiClient client;

void loop() {
    if (!client || !client.connected()) {
        client = server.accept();
    }

    MessageHeader header(0, 0);
    if (client && client.available() >= sizeof(MessageHeader)) {
        ParseMessageResult result = readMessage(client, header, message_buf);
        if (result == ParseMessageResult::Success) {
            header.debugPrint();
        } else {
            Serial.print("Failed to read message: ");
            switch (result) {
            case ParseMessageResult::NotEnoughData:
                Serial.println("Not enough data");
                break;
            case ParseMessageResult::InvalidKind:
                Serial.println("Invalid kind");
                break;
            case ParseMessageResult::InvalidSize:
                Serial.println("Invalid size");
                break;
            default:
                Serial.println("Unknown error");
                break;
            }
        }
    }

    if (Serial.available()) {
        size_t bytes_read = Serial.readBytes(serial_buf, sizeof(serial_buf));

        if (bytes_read > 0 && client && client.connected()) {
            MessageHeader header(MessageKindData, bytes_read);
            sendMessage(client, header, serial_buf);
        }
    }
}
