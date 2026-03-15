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

    server.begin();
    server.setNoDelay(true);
}

WiFiClient client;

SerialConfig cfgToSerialConfig(MessageConfigData* cfg) {
    if (cfg->data_bits == 5) {
        if (cfg->parity == 0) {
            return cfg->stop_bits == 1 ? SERIAL_5N1 : SERIAL_5N2;
        } else if (cfg->parity == 1) {
            return cfg->stop_bits == 1 ? SERIAL_5E1 : SERIAL_5E2;
        } else if (cfg->parity == 2) {
            return cfg->stop_bits == 1 ? SERIAL_5O1 : SERIAL_5O2;
        }
    } else if (cfg->data_bits == 6) {
        if (cfg->parity == 0) {
            return cfg->stop_bits == 1 ? SERIAL_6N1 : SERIAL_6N2;
        } else if (cfg->parity == 1) {
            return cfg->stop_bits == 1 ? SERIAL_6E1 : SERIAL_6E2;
        } else if (cfg->parity == 2) {
            return cfg->stop_bits == 1 ? SERIAL_6O1 : SERIAL_6O2;
        }
    } else if (cfg->data_bits == 7) {
        if (cfg->parity == 0) {
            return cfg->stop_bits == 1 ? SERIAL_7N1 : SERIAL_7N2;
        } else if (cfg->parity == 1) {
            return cfg->stop_bits == 1 ? SERIAL_7E1 : SERIAL_7E2;
        } else if (cfg->parity == 2) {
            return cfg->stop_bits == 1 ? SERIAL_7O1 : SERIAL_7O2;
        }
    } else if (cfg->data_bits == 8) {
        if (cfg->parity == 0) {
            return cfg->stop_bits == 1 ? SERIAL_8N1 : SERIAL_8N2;
        } else if (cfg->parity == 1) {
            return cfg->stop_bits == 1 ? SERIAL_8E1 : SERIAL_8E2;
        } else if (cfg->parity == 2) {
            return cfg->stop_bits == 1 ? SERIAL_8O1 : SERIAL_8O2;
        }
    }

    return SERIAL_8N1;
}

void handle_message(MessageHeader header, uint8_t* buf) {
    if (header.kind == MessageKindData) {
        Serial.write(buf, header.size);
    } else if (header.kind == MessageKindConfig) {
        MessageConfigData* cfg = (MessageConfigData*) buf;
        cfg->baud = ntohl(cfg->baud);
        Serial.flush();
        Serial.begin(cfg->baud, cfgToSerialConfig(cfg));
    }
}

uint8_t message_buf[MAX_MESSAGE_SIZE];
uint8_t serial_buf[128];

void loop() {
    if (!client || !client.connected()) {
        client = server.accept();
    }

    MessageHeader header(0, 0);
    if (client && client.available() >= sizeof(MessageHeader)) {
        ParseMessageResult result = readMessage(client, header, message_buf);
        if (result == ParseMessageResult::Success) {
            handle_message(header, message_buf);
        }
    }

    if (Serial.available() && client && client.connected()) {
        size_t bytes_to_read = Serial.available();
        if (bytes_to_read > sizeof(serial_buf)) {
            bytes_to_read = sizeof(serial_buf);
        }

        if (bytes_to_read > 0) {
            for (size_t i = 0; i < bytes_to_read; i++) {
                serial_buf[i] = Serial.read();
            }

            MessageHeader header(MessageKindData, bytes_to_read);
            sendMessage(client, header, serial_buf);
        }
    }
}
