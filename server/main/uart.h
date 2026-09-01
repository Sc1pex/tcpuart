#pragma once

#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"

typedef struct {
    QueueHandle_t tcp_to_uart_queue;
    QueueHandle_t uart_to_tcp_queue;
    int uart_to_tcp_efd;

#ifdef CONFIG_ESP_STATUS_LED_ENABLED
    QueueHandle_t status_update_queue;
#endif
} UartTaskParams;

void uart_task(void* pvParameters);
