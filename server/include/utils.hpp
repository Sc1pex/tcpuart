#pragma once

#include <ESP8266WiFi.h>

int read_all(WiFiClient& client, uint8_t* buffer, size_t size);