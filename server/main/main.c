#include "esp_vfs_eventfd.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/task.h"
#include "message.h"
#include "status.h"
#include "tcp.h"
#include "uart.h"
#include "wifi.h"

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
    state->uart_to_tcp_queue = xQueueCreate(16, sizeof(Message));
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

    xTaskCreatePinnedToCore(uart_task, "uart_task", 4096, &s_state.uart_params, 5, NULL, 1);
    xTaskCreatePinnedToCore(tcp_task, "tcp_task", 4096, &s_state.tcp_params, 5, NULL, 0);
}
