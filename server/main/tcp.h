#pragma once

#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"

typedef struct {
    QueueHandle_t tcp_to_uart_queue;
} TcpTaskParams;

void tcp_task(void* pvParameters);
