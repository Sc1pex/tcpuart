#include "status.h"
#include <string.h>
#include "led_strip.h"

void set_color(led_strip_handle_t handle, uint32_t red, uint32_t green, uint32_t blue) {
    led_strip_set_pixel(
        handle, 0, red / CONFIG_ESP_STATUS_LED_BRIGHTNESS, green / CONFIG_ESP_STATUS_LED_BRIGHTNESS,
        blue / CONFIG_ESP_STATUS_LED_BRIGHTNESS
    );
    led_strip_refresh(handle);
}

void status_led_task(void* pvParameters) {
    StatusTaskParams* params = (StatusTaskParams*) pvParameters;

    led_strip_config_t led_config = {
        .strip_gpio_num = CONFIG_ESP_STATUS_LED_GPIO,
        .max_leds = 1,
    };
    led_strip_rmt_config_t rmt_config;
    memset(&rmt_config, 0, sizeof(rmt_config));

    led_strip_handle_t led_handle;
    ESP_ERROR_CHECK(led_strip_new_rmt_device(&led_config, &rmt_config, &led_handle));

    StatusUpdateMessage msg;
    while (xQueueReceive(params->status_update_queue, &msg, portMAX_DELAY) == pdTRUE) {
        if (msg == STATUS_WIFI_CONNECTING) {
            set_color(led_handle, 255, 255, 0);
            vTaskDelay(pdMS_TO_TICKS(250));
            set_color(led_handle, 0, 0, 0);
        } else if (msg == STATUS_WIFI_CONNECTED) {
            set_color(led_handle, 0, 255, 0);
            vTaskDelay(pdMS_TO_TICKS(1000));
            set_color(led_handle, 0, 0, 0);
        } else if (msg == STATUS_WIFI_CONNECTION_FAILED) {
            set_color(led_handle, 255, 0, 0);
            vTaskDelay(pdMS_TO_TICKS(250));
            set_color(led_handle, 0, 0, 0);
        } else if (msg == STATUS_WIFI_RETRIES_FAILED) {
            set_color(led_handle, 255, 0, 0);
            vTaskDelay(pdMS_TO_TICKS(3000));
            set_color(led_handle, 0, 0, 0);
        }
    }
}
