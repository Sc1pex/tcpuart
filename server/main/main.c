#include "esp_log.h"
#include "esp_vfs_eventfd.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/task.h"
#include "message.h"
#include "sdkconfig.h"
#include "status.h"
#include "tcp.h"
#include "uart.h"
#include "wifi.h"

#ifdef CONFIG_ESP_TAILSCALE_ENABLED
#include "microlink.h"
#endif

static const char* TAG = "main";

typedef struct {
    QueueHandle_t tcp_to_uart_queue;
    QueueHandle_t uart_to_tcp_queue;
    int uart_to_tcp_efd;
    UartTaskParams uart_params;
    TcpTaskParams tcp_params;

#ifdef CONFIG_ESP_STATUS_LED_ENABLED
    QueueHandle_t status_update_queue;
    StatusTaskParams status_params;
#endif
} AppState;

static AppState s_state;

static void state_init(AppState* state) {
    esp_vfs_eventfd_config_t cfg = ESP_VFS_EVENTD_CONFIG_DEFAULT();
    ESP_ERROR_CHECK(esp_vfs_eventfd_register(&cfg));

    state->tcp_to_uart_queue = xQueueCreate(16, sizeof(Message));
    // Depth 128 absorbs VPN jitter bursts without stalling uart_rx_task.
    state->uart_to_tcp_queue = xQueueCreate(128, sizeof(Message));
    state->uart_to_tcp_efd = eventfd(0, 0);

    // Initialize task parameters with the shared resources
    state->uart_params.tcp_to_uart_queue = state->tcp_to_uart_queue;
    state->uart_params.uart_to_tcp_queue = state->uart_to_tcp_queue;
    state->uart_params.uart_to_tcp_efd = state->uart_to_tcp_efd;

    state->tcp_params.tcp_to_uart_queue = state->tcp_to_uart_queue;
    state->tcp_params.uart_to_tcp_queue = state->uart_to_tcp_queue;
    state->tcp_params.uart_to_tcp_efd = state->uart_to_tcp_efd;

#ifdef CONFIG_ESP_STATUS_LED_ENABLED
    state->status_update_queue = xQueueCreate(2, sizeof(StatusUpdateMessage));

    state->status_params.status_update_queue = state->status_update_queue;

    state->uart_params.status_update_queue = state->status_update_queue;
    state->tcp_params.status_update_queue = state->status_update_queue;
#endif
}
void app_main(void) {
    state_init(&s_state);

    WifiParams wifi_params;
#ifdef CONFIG_ESP_STATUS_LED_ENABLED
    xTaskCreate(status_led_task, "status_task", 4096, &s_state.status_params, 1, NULL);

    wifi_params.status_update_queue = s_state.status_update_queue;
#endif

    wifi_init(&wifi_params);

#ifdef CONFIG_ESP_TAILSCALE_ENABLED
    ESP_LOGI(TAG, "Initializing Tailscale (MicroLink)...");
#ifdef CONFIG_ESP_STATUS_LED_ENABLED
    StatusUpdateMessage status_msg = STATUS_TAILSCALE_CONNECTING;
    xQueueSend(s_state.status_update_queue, &status_msg, 0);
#endif

    microlink_config_t ml_cfg = {
        .auth_key = CONFIG_ESP_TAILSCALE_AUTH_KEY,
        .device_name = CONFIG_ESP_TAILSCALE_DEVICE_NAME,
        .enable_derp = true,
        .enable_disco = true,
        .enable_stun = true,
    };
    microlink_t* ml = microlink_init(&ml_cfg);
    if (!ml) {
        ESP_LOGE(TAG, "Failed to initialize MicroLink! Rebooting...");
#ifdef CONFIG_ESP_STATUS_LED_ENABLED
        status_msg = STATUS_TAILSCALE_FAILED;
        xQueueSend(s_state.status_update_queue, &status_msg, 0);
#endif
        vTaskDelay(pdMS_TO_TICKS(5000));
        esp_restart();
    }
    microlink_start(ml);

    ESP_LOGI(TAG, "Waiting for Tailscale VPN connection...");
    int timeout_sec = 60;
    while (!microlink_is_connected(ml) && timeout_sec > 0) {
        vTaskDelay(pdMS_TO_TICKS(1000));
        timeout_sec--;
    }

    if (!microlink_is_connected(ml)) {
        ESP_LOGE(TAG, "Failed to connect to Tailscale network within timeout. Rebooting...");
#ifdef CONFIG_ESP_STATUS_LED_ENABLED
        status_msg = STATUS_TAILSCALE_FAILED;
        xQueueSend(s_state.status_update_queue, &status_msg, 0);
#endif
        vTaskDelay(pdMS_TO_TICKS(5000));
        esp_restart();
    }

    uint32_t vpn_ip = microlink_get_vpn_ip(ml);
    char ip_str[16];
    microlink_ip_to_str(vpn_ip, ip_str);
    ESP_LOGI(TAG, "Tailscale connected! VPN IP: %s", ip_str);

#ifdef CONFIG_ESP_STATUS_LED_ENABLED
    status_msg = STATUS_TAILSCALE_CONNECTED;
    xQueueSend(s_state.status_update_queue, &status_msg, 0);
#endif
#endif

    xTaskCreatePinnedToCore(uart_rx_task, "uart_rx_task", 4096, &s_state.uart_params, 8, NULL, 1);
    xTaskCreatePinnedToCore(uart_tx_task, "uart_tx_task", 4096, &s_state.uart_params, 8, NULL, 1);
    xTaskCreatePinnedToCore(tcp_task, "tcp_task", 4096, &s_state.tcp_params, 5, NULL, 0);
}
