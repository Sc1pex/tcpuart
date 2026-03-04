#include "utils.hpp"

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