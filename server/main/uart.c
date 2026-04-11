#include "uart.h"
#include "driver/gpio.h"
#include "driver/uart.h"
#include "esp_log.h"
#include "esp_vfs.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/task.h"
#include "message.h"

static const char* TAG = "uart";

#define UART_EVENT_QUEUE_SIZE 16

void uart_task(void* pvParamters) {
    UartTaskParams* params = (UartTaskParams*) pvParamters;
    uart_config_t cfg = {
        .baud_rate = 115200,
        .data_bits = UART_DATA_8_BITS,
        .stop_bits = UART_STOP_BITS_1,
        .flow_ctrl = UART_HW_FLOWCTRL_DISABLE,
        .source_clk = UART_SCLK_DEFAULT,
        .parity = UART_PARITY_DISABLE,
    };

    QueueHandle_t uart_event_queue;
    ESP_ERROR_CHECK(
        uart_driver_install(UART_NUM_2, 1024, 1024, UART_EVENT_QUEUE_SIZE, &uart_event_queue, 0)
    );
    ESP_ERROR_CHECK(uart_param_config(UART_NUM_2, &cfg));
    ESP_ERROR_CHECK(
        uart_set_pin(UART_NUM_2, GPIO_NUM_17, GPIO_NUM_16, UART_PIN_NO_CHANGE, UART_PIN_NO_CHANGE)
    );

    QueueSetHandle_t queue_set = xQueueCreateSet(16 + UART_EVENT_QUEUE_SIZE);
    xQueueAddToSet(uart_event_queue, queue_set);
    xQueueAddToSet(params->tcp_to_uart_queue, queue_set);

    while (1) {
        QueueSetMemberHandle_t active_queue = xQueueSelectFromSet(queue_set, portMAX_DELAY);

        if (active_queue == params->tcp_to_uart_queue) {
            Message msg;
            if (xQueueReceive(params->tcp_to_uart_queue, &msg, 0) == pdTRUE) {
                if (msg.hdr.kind == MESSAGE_KIND_DATA) {
                    uart_write_bytes(UART_NUM_2, msg.body, msg.hdr.len);
                } else if (msg.hdr.kind == MESSAGE_KIND_CONFIG) {
                    // TODO: handle config message
                }
            }
        } else if (active_queue == uart_event_queue) {
            uart_event_t event;
            if (xQueueReceive(uart_event_queue, &event, 0) == pdTRUE) {
                if (event.type == UART_DATA) {
                    size_t buffered_len;
                    uart_get_buffered_data_len(UART_NUM_2, &buffered_len);

                    while (buffered_len > 0) {
                        Message msg;
                        msg.hdr.kind = MESSAGE_KIND_DATA;
                        size_t to_read = (buffered_len > MAX_MESSAGE_BODY_SIZE)
                                             ? MAX_MESSAGE_BODY_SIZE
                                             : buffered_len;

                        int len = uart_read_bytes(UART_NUM_2, msg.body, to_read, 0);
                        if (len > 0) {
                            // Log the received data for debugging
                            ESP_LOGI(TAG, "Received %d bytes from UART: %.*s", len, len, msg.body);
                            msg.hdr.len = len;
                            // Block indefinitely until the TCP task has room for this data
                            xQueueSend(params->uart_to_tcp_queue, &msg, portMAX_DELAY);

                            // Signal the TCP task that data is ready
                            uint64_t val = 1;
                            write(params->uart_to_tcp_efd, &val, sizeof(val));
                        }
                        uart_get_buffered_data_len(UART_NUM_2, &buffered_len);
                    }
                }
            }
        }
    }
}
