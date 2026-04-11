#include "tcp.h"
#include "esp_log.h"
#include "esp_netif_ip_addr.h"
#include "freertos/FreeRTOS.h"
#include "freertos/idf_additions.h"
#include "freertos/queue.h"
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

void handle_client(int client_sock, TcpTaskParams* params) {
    int opt = 1;
    setsockopt(client_sock, IPPROTO_TCP, TCP_NODELAY, &opt, sizeof(opt));

    while (1) {
        fd_set readfds;
        FD_ZERO(&readfds);

        FD_SET(client_sock, &readfds);
        FD_SET(params->uart_to_tcp_efd, &readfds);
        int maxfd = client_sock > params->uart_to_tcp_efd ? client_sock : params->uart_to_tcp_efd;

        int num_ready = select(maxfd + 1, &readfds, NULL, NULL, NULL);
        if (num_ready < 0) {
            ESP_LOGE(TAG, "select failed");
            break;
        }

        if (FD_ISSET(client_sock, &readfds)) {
            Message msg;
            int ret = read_msg(client_sock, &msg);
            if (ret < 0) {
                ESP_LOGE(TAG, "failed to read message from client");
                break;
            }
            if (xQueueSend(params->tcp_to_uart_queue, &msg, 0) != pdTRUE) {
                ESP_LOGE(TAG, "Failed to send message to UART queue");
            }
        }

        if (FD_ISSET(params->uart_to_tcp_efd, &readfds)) {
            uint64_t val;
            read(params->uart_to_tcp_efd, &val, sizeof(val));

            Message msg;
            while (xQueueReceive(params->uart_to_tcp_queue, &msg, 0) == pdTRUE) {
                if (send(client_sock, &msg.hdr, sizeof(Header), 0) < 0) {
                    ESP_LOGE(TAG, "send header failed");
                    close(client_sock);
                    return;
                }
                if (send(client_sock, msg.body, msg.hdr.len, 0) < 0) {
                    ESP_LOGE(TAG, "send body failed");
                    close(client_sock);
                    return;
                }
            }
        }
    }
}

void tcp_task(void* pvParamters) {
    TcpTaskParams* params = (TcpTaskParams*) pvParamters;

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
        handle_client(client_sock, params);
    }
}
