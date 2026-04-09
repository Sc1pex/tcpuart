#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/task.h"
#include "message.h"
#include "tcp.h"
#include "uart.h"
#include "wifi.h"

void app_main(void) {
    wifi_init();

    // Create queue for TCP → UART communication
    QueueHandle_t tcp_to_uart_queue = xQueueCreate(16, sizeof(Message));

    // Setup task parameters
    static UartTaskParams uart_params = { 0 };
    uart_params.tcp_to_uart_queue = tcp_to_uart_queue;

    static TcpTaskParams tcp_params = { 0 };
    tcp_params.tcp_to_uart_queue = tcp_to_uart_queue;

    xTaskCreatePinnedToCore(uart_task, "uart_task", 4096, &uart_params, 5, NULL, 1);
    xTaskCreatePinnedToCore(tcp_task, "tcp_task", 4096, &tcp_params, 5, NULL, 0);
}
