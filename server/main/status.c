#include "status.h"
#include <string.h>
#include "freertos/projdefs.h"
#include "led_strip.h"

led_strip_handle_t s_led_handle;

uint32_t s_red_delay_ms = 0;
uint32_t s_green_delay_ms = 0;
uint32_t s_blue_delay_ms = 0;

uint32_t s_red_value = 0;
uint32_t s_green_value = 0;
uint32_t s_blue_value = 0;

void update_color() {
    led_strip_set_pixel(
        s_led_handle, 0, (s_red_value * CONFIG_ESP_STATUS_LED_BRIGHTNESS) / 100,
        (s_green_value * CONFIG_ESP_STATUS_LED_BRIGHTNESS) / 100,
        (s_blue_value * CONFIG_ESP_STATUS_LED_BRIGHTNESS) / 100
    );
    led_strip_refresh(s_led_handle);
}

void handle_queue_msg(StatusUpdateMessage msg) {
    if (msg == STATUS_WIFI_CONNECTING) {
        s_red_value = 255;
        s_red_delay_ms = 250;

        s_green_value = 255;
        s_green_delay_ms = 250;
    } else if (msg == STATUS_WIFI_CONNECTED) {
        s_green_value = 255;
        s_green_delay_ms = 250;
    } else if (msg == STATUS_WIFI_CONNECTION_FAILED) {
        s_red_value = 255;
        s_red_delay_ms = 250;
    } else if (msg == STATUS_WIFI_RETRIES_FAILED) {
        s_red_value = 255;
        s_red_delay_ms = 3000;
    }

    else if (msg == STATUS_TCP_CLIENT_CONNECTED) {
        s_blue_value = 255;
        s_blue_delay_ms = 500;
    } else if (msg == STATUS_TCP_CLIENT_DISCONNECTED) {
        s_blue_value = 255;
        s_blue_delay_ms = 500;
    }

    else if (msg == STATUS_UART_SEND) {
        s_red_value = 255;
        s_red_delay_ms = 10;
    } else if (msg == STATUS_UART_RECV) {
        s_green_value = 255;
        s_green_delay_ms = 10;
    }
}

uint32_t min(uint32_t a, uint32_t b) {
    if (a < b) {
        return a;
    } else {
        return b;
    }
}
void update_delays(uint32_t elapsed_ms) {
    s_red_delay_ms -= min(elapsed_ms, s_red_delay_ms);
    s_green_delay_ms -= min(elapsed_ms, s_green_delay_ms);
    s_blue_delay_ms -= min(elapsed_ms, s_blue_delay_ms);

    if (s_red_delay_ms == 0) {
        s_red_value = 0;
    }
    if (s_green_delay_ms == 0) {
        s_green_value = 0;
    }
    if (s_blue_delay_ms == 0) {
        s_blue_value = 0;
    }
}

uint32_t calculate_queue_wait() {
    if (s_red_delay_ms == 0 && s_green_delay_ms == 0 && s_blue_delay_ms == 0) {
        return portMAX_DELAY;
    }

    int ms_wait =
        min(min((s_red_delay_ms ? s_red_delay_ms : UINT32_MAX),
                (s_green_delay_ms ? s_green_delay_ms : UINT32_MAX)),
            (s_blue_delay_ms ? s_blue_delay_ms : UINT32_MAX));
    return pdMS_TO_TICKS(ms_wait);
}

void status_led_task(void* pvParameters) {
    StatusTaskParams* params = (StatusTaskParams*) pvParameters;

    led_strip_config_t led_config = {
        .strip_gpio_num = CONFIG_ESP_STATUS_LED_GPIO,
        .max_leds = 1,
    };
    led_strip_rmt_config_t rmt_config;
    memset(&rmt_config, 0, sizeof(rmt_config));

    ESP_ERROR_CHECK(led_strip_new_rmt_device(&led_config, &rmt_config, &s_led_handle));

    StatusUpdateMessage msg;

    while (1) {
        int queue_wait = calculate_queue_wait();

        uint32_t start_wait = xTaskGetTickCount();
        int received = xQueueReceive(params->status_update_queue, &msg, queue_wait) == pdTRUE;

        uint32_t end_wait = xTaskGetTickCount();
        uint32_t elapsed_ms = pdTICKS_TO_MS(end_wait - start_wait);
        update_delays(elapsed_ms);

        if (received == pdTRUE) {
            handle_queue_msg(msg);
        }

        update_color();
    }
}
