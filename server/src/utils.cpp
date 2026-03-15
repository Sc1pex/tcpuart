#include "utils.hpp"

int read_all(WiFiClient& client, uint8_t* buffer, size_t size) {
    size_t total_read = 0;
    while (total_read < size) {
        if (!client.connected()) {
            return -1;
        }
        if (client.available()) {
            int ret = client.read(buffer + total_read, size - total_read);
            if (ret > 0) {
                total_read += ret;
            } else if (ret < 0) {
                return -1;
            }
        }
    }

    return 0;
}