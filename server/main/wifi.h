#pragma once

#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"

typedef struct {
#ifdef CONFIG_ESP_STATUS_LED_ENABLED
    QueueHandle_t status_update_queue;
#endif
} WifiParams;

void wifi_init(WifiParams* params);
