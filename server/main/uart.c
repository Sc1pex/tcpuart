#include "uart.h"
#include "driver/gpio.h"
#include "driver/uart.h"
#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

static const char* TAG = "uart";

void uart_task() {
    uart_config_t cfg = {
        .baud_rate = 115200,
        .data_bits = UART_DATA_8_BITS,
        .stop_bits = UART_STOP_BITS_1,
        .flow_ctrl = UART_HW_FLOWCTRL_DISABLE,
        .source_clk = UART_SCLK_DEFAULT,
        .parity = UART_PARITY_DISABLE,
    };

    ESP_ERROR_CHECK(uart_driver_install(UART_NUM_2, 1024, 1024, 0, NULL, 0));
    ESP_ERROR_CHECK(uart_param_config(UART_NUM_2, &cfg));
    ESP_ERROR_CHECK(
        uart_set_pin(UART_NUM_2, GPIO_NUM_17, GPIO_NUM_16, UART_PIN_NO_CHANGE, UART_PIN_NO_CHANGE)
    );

    uint8_t* data = (uint8_t*) malloc(1024);
    while (1) {
        int len = uart_read_bytes(UART_NUM_2, data, 1024, 20 / portTICK_PERIOD_MS);
        if (len > 0) {
            ESP_LOGI(TAG, "Read %d bytes from UART: %.*s", len, len, data);
        }
    }
}

void start_uart_task() {
    xTaskCreate(uart_task, "uart_task", 4096, NULL, 5, NULL);
}
