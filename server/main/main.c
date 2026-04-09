#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/task.h"
#include "message.h"
#include "tcp.h"
#include "uart.h"
#include "wifi.h"

typedef struct {
    QueueHandle_t tcp_to_uart_queue;
    UartTaskParams uart_params;
    TcpTaskParams tcp_params;
} AppState;

static AppState s_state;

static void state_init(AppState* state) {
    state->tcp_to_uart_queue = xQueueCreate(16, sizeof(Message));

    state->uart_params.tcp_to_uart_queue = state->tcp_to_uart_queue;
    state->tcp_params.tcp_to_uart_queue = state->tcp_to_uart_queue;
}

void app_main(void) {
    state_init(&s_state);

    wifi_init();

    xTaskCreatePinnedToCore(uart_task, "uart_task", 4096, &s_state.uart_params, 5, NULL, 1);
    xTaskCreatePinnedToCore(tcp_task, "tcp_task", 4096, &s_state.tcp_params, 5, NULL, 0);
}
