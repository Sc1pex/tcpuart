#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

static const char* TAG = "server";

void app_main(void) {
    ESP_LOGI(TAG, "Hello, world");

    while (1) {
        ESP_LOGI(TAG, "tick");
        vTaskDelay(pdMS_TO_TICKS(1000));
    }
}
