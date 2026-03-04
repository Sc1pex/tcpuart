#include <Arduino.h>
#include <ESP8266WiFi.h>
#include <WiFiClient.h>
#include "message.hpp"

WiFiServer server(15113);

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
            Serial.printf("message: %.*s\n", header.size, message_buf);
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
        size_t bytes_to_read = Serial.available();
        if (bytes_to_read > sizeof(serial_buf)) {
            bytes_to_read = sizeof(serial_buf);
        }

        for (size_t i = 0; i < bytes_to_read; i++) {
            serial_buf[i] = Serial.read();
        }

        Serial.printf("serial_buf: %.*s\n", bytes_to_read, serial_buf);

        if (bytes_to_read > 0 && client && client.connected()) {
            MessageHeader header(MessageKindData, bytes_to_read);
            sendMessage(client, header, serial_buf);
        }
    }
}
