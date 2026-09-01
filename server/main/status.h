#pragma once

#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"

typedef enum {
    STATUS_WIFI_CONNECTING,
    STATUS_WIFI_CONNECTED,
    STATUS_WIFI_CONNECTION_FAILED,
    STATUS_WIFI_RETRIES_FAILED,
} StatusUpdateMessage;

typedef struct {
    QueueHandle_t status_update_queue;
} StatusTaskParams;

void status_led_task(void* pvParameters);
