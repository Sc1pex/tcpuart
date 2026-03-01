#include <Arduino.h>
#include <ESP8266WiFi.h>
#include <WiFiClient.h>

#define MAX_MESSAGE_SIZE 1024

WiFiServer server(15113);

enum MessageKind {
    MessageKindData,
    MessageKindConfig,
};

struct MessageHeader {
    uint16_t kind;
    uint16_t size;

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

uint8_t header_buf[4];
uint8_t message_buf[MAX_MESSAGE_SIZE];

void loop() {
    WiFiClient client = server.accept();

    if (client) {
        Serial.println("Client connected");
        while (client.connected()) {
            if (client.available()) {
                read_all(client, header_buf, sizeof(header_buf));
                MessageHeader header(header_buf);
                header.debugPrint();

                if (header.size > MAX_MESSAGE_SIZE) {
                    Serial.println("Message size exceeds maximum, disconnecting client");
                    break;
                }

                read_all(client, message_buf, header.size);
                Serial.print("Received message: ");
                Serial.write(message_buf, header.size);
                Serial.println();
            }
        }
        Serial.println("Client disconnected");
    }
}
