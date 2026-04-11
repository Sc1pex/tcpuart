#pragma once

#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"

typedef struct {
    QueueHandle_t tcp_to_uart_queue;
    QueueHandle_t uart_to_tcp_queue;
    int uart_to_tcp_efd;
} TcpTaskParams;

void tcp_task(void* pvParameters);
