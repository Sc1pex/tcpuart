#include "esp_log.h"
#include "esp_netif_ip_addr.h"
#include "freertos/FreeRTOS.h"
#include "freertos/idf_additions.h"
#include "lwip/sockets.h"
#include "message.h"

#define PORT CONFIG_ESP_TCP_SERVER_PORT

static const char* TAG = "tcp server";

int read_all(char* buf, size_t size, int sock) {
    size_t total_read = 0;
    while (total_read < size) {
        ssize_t bytes_read = recv(sock, buf + total_read, size - total_read, 0);
        if (bytes_read <= 0) {
            if (bytes_read == 0) {
                ESP_LOGE(TAG, "connection closed");
            } else {
                ESP_LOGE(TAG, "recv failed");
            }
            return -1;
        } else {
            ESP_LOGD(TAG, "read %d bytes (total %d/%d)", bytes_read, total_read + bytes_read, size);
        }
        total_read += bytes_read;
    }
    return 0;
}

int read_msg(int sock, Message* msg) {
    if (read_all((char*) &msg->hdr, sizeof(Header), sock) < 0) {
        ESP_LOGE(TAG, "failed to read message header");
        return -1;
    }
    ESP_LOGD(TAG, "read message header: kind=%d, len=%d", msg->hdr.kind, msg->hdr.len);
    if (read_all((char*) msg->body, msg->hdr.len, sock) < 0) {
        ESP_LOGE(TAG, "failed to read message body");
        return -1;
    }
    return 0;
}

void debug_print_message(const Message* msg) {
    ESP_LOGI(TAG, "Message kind: %d", msg->hdr.kind);
    ESP_LOGI(TAG, "Message length: %d", msg->hdr.len);
    if (msg->hdr.kind == MESSAGE_KIND_DATA) {
        ESP_LOGI(TAG, "Data: %.*s", msg->hdr.len, msg->body);
    } else if (msg->hdr.kind == MESSAGE_KIND_CONFIG) {
        if (msg->hdr.len != sizeof(configmessage)) {
            ESP_LOGE(TAG, "Invalid config message length");
            return;
        }
        const configmessage* cfg = (const configmessage*) msg->body;
        ESP_LOGI(
            TAG, "Config - Baudrate: %u, Data bits: %u, Stop bits: %u, Parity: %u",
            ntohl(cfg->baudrate), cfg->data_bits, cfg->stop_bits, cfg->parity
        );
    } else {
        ESP_LOGW(TAG, "Unknown message kind");
    }
}

void handle_client(int client_sock) {
    Message msg;

    while (1) {
        if (read_msg(client_sock, &msg) < 0) {
            close(client_sock);
            return;
        }
        debug_print_message(&msg);
    }
}

void tcp_task() {
    int sock = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    if (sock < 0) {
        ESP_LOGE(TAG, "failed to create socket");
        vTaskDelete(NULL);
        return;
    }

    int opt = 1;
    setsockopt(sock, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));

    struct sockaddr_in addr = {
        .sin_family = AF_INET,
        .sin_port = htons(PORT),
        .sin_addr.s_addr = INADDR_ANY,
    };

    if (bind(sock, (struct sockaddr*) &addr, sizeof(addr)) < 0) {
        ESP_LOGE(TAG, "bind failed");
        close(sock);
        vTaskDelete(NULL);
        return;
    }

    listen(sock, 1);
    ESP_LOGI(TAG, "listening on port %d", PORT);

    while (1) {
        struct sockaddr_in client_addr;
        socklen_t client_len = sizeof(client_addr);

        int client_sock = accept(sock, (struct sockaddr*) &client_addr, &client_len);
        if (client_sock < 0) {
            ESP_LOGE(TAG, "accept failed");
            continue;
        }

        ESP_LOGI(TAG, "client connected: " IPSTR, IP2STR((ip4_addr_t*) &client_addr.sin_addr));
        handle_client(client_sock);
    }
}

void start_tcp_task() {
    xTaskCreate(tcp_task, "tcp_server", 4096, NULL, 5, NULL);
}
