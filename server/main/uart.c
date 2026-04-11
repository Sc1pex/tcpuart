#include "uart.h"
#include "driver/gpio.h"
#include "driver/uart.h"
#include "esp_log.h"
#include "esp_vfs.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/task.h"
#include "lwip/sockets.h"
#include "message.h"

static const char* TAG = "uart";

#define UART_PORT CONFIG_ESP_UART_PORT_NUM
#define UART_EVENT_QUEUE_SIZE 16

#ifdef CONFIG_ESP_UART_RESET_ENABLED
#define RESET_PIN CONFIG_ESP_UART_RESET_PIN

#ifdef CONFIG_ESP_UART_RESET_ACTIVE_LOW
#define RESET_ACTIVE_LEVEL 0
#else
#define RESET_ACTIVE_LEVEL 1
#endif

#define RESET_INACTIVE_LEVEL (1 - RESET_ACTIVE_LEVEL)
#endif

void apply_config(const ConfigMessage* config) {
    ESP_LOGI(
        TAG, "Applying UART config: baud_rate=%u, data_bits=%d, stop_bits=%d, parity=%d",
        ntohl(config->baud_rate), config->data_bits, config->stop_bits, config->parity
    );
    uint32_t baud_rate = ntohl(config->baud_rate);
    ESP_ERROR_CHECK(uart_set_baudrate(UART_PORT, baud_rate));

    switch (config->data_bits) {
    case 5:
        ESP_ERROR_CHECK(uart_set_word_length(UART_PORT, UART_DATA_5_BITS));
        break;
    case 6:
        ESP_ERROR_CHECK(uart_set_word_length(UART_PORT, UART_DATA_6_BITS));
        break;
    case 7:
        ESP_ERROR_CHECK(uart_set_word_length(UART_PORT, UART_DATA_7_BITS));
        break;
    case 8:
        ESP_ERROR_CHECK(uart_set_word_length(UART_PORT, UART_DATA_8_BITS));
        break;
    default:
        ESP_LOGE(TAG, "invalid data bits: %d", config->data_bits);
        break;
    }

    switch (config->stop_bits) {
    case 1:
        ESP_ERROR_CHECK(uart_set_stop_bits(UART_PORT, UART_STOP_BITS_1));
        break;
    case 2:
        ESP_ERROR_CHECK(uart_set_stop_bits(UART_PORT, UART_STOP_BITS_2));
        break;
    default:
        ESP_LOGE(TAG, "invalid stop bits: %d", config->stop_bits);
        break;
    }

    switch (config->parity) {
    case 0:
        ESP_ERROR_CHECK(uart_set_parity(UART_PORT, UART_PARITY_DISABLE));
        break;
    case 1:
        ESP_ERROR_CHECK(uart_set_parity(UART_PORT, UART_PARITY_ODD));
        break;
    case 2:
        ESP_ERROR_CHECK(uart_set_parity(UART_PORT, UART_PARITY_EVEN));
        break;
    default:
        ESP_LOGE(TAG, "invalid parity: %d", config->parity);
        break;
    }
}

static void handle_control(const ControlMessage* ctrl, UartTaskParams* params) {
    Message resp_msg;
    resp_msg.hdr.kind = MESSAGE_KIND_RESPONSE;
    resp_msg.hdr.len = sizeof(ResponseMessage);
    ResponseMessage* resp = (ResponseMessage*) resp_msg.body;

    if (ctrl->command == CONTROL_CMD_RESET) {
#ifdef CONFIG_ESP_UART_RESET_ENABLED
        ESP_LOGI(TAG, "Performing remote reset on GPIO %d", RESET_PIN);
        gpio_set_level(RESET_PIN, RESET_ACTIVE_LEVEL);
        vTaskDelay(pdMS_TO_TICKS(CONFIG_ESP_UART_RESET_DURATION_MS));
        gpio_set_level(RESET_PIN, RESET_INACTIVE_LEVEL);
        resp->status = RESPONSE_STATUS_OK;
#else
        ESP_LOGW(TAG, "Remote reset command received but feature is disabled");
        resp->status = RESPONSE_STATUS_NOT_SUPPORTED;
#endif
    } else {
        ESP_LOGW(TAG, "Unknown control command: %d", ctrl->command);
        resp->status = RESPONSE_STATUS_NOT_SUPPORTED;
    }

    if (xQueueSend(params->uart_to_tcp_queue, &resp_msg, portMAX_DELAY) == pdTRUE) {
        uint64_t val = 1;
        write(params->uart_to_tcp_efd, &val, sizeof(val));
    }
}

void uart_task(void* pvParamters) {
    UartTaskParams* params = (UartTaskParams*) pvParamters;

#ifdef CONFIG_ESP_UART_RESET_ENABLED
    gpio_config_t reset_gpio_cfg = {
        .pin_bit_mask = (1ULL << RESET_PIN),
        .mode = GPIO_MODE_OUTPUT,
        .pull_up_en = GPIO_PULLUP_DISABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type = GPIO_INTR_DISABLE,
    };
    ESP_ERROR_CHECK(gpio_config(&reset_gpio_cfg));
    ESP_ERROR_CHECK(gpio_set_level(RESET_PIN, RESET_INACTIVE_LEVEL));
    ESP_LOGI(TAG, "Reset GPIO %d initialized to level %d", RESET_PIN, RESET_INACTIVE_LEVEL);
#endif

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
        uart_driver_install(UART_PORT, 1024, 1024, UART_EVENT_QUEUE_SIZE, &uart_event_queue, 0)
    );
    ESP_ERROR_CHECK(uart_param_config(UART_PORT, &cfg));
    ESP_ERROR_CHECK(uart_set_pin(
        UART_PORT, CONFIG_ESP_UART_TX_PIN, CONFIG_ESP_UART_RX_PIN, UART_PIN_NO_CHANGE,
        UART_PIN_NO_CHANGE
    ));

    QueueSetHandle_t queue_set = xQueueCreateSet(16 + UART_EVENT_QUEUE_SIZE);
    xQueueAddToSet(uart_event_queue, queue_set);
    xQueueAddToSet(params->tcp_to_uart_queue, queue_set);

    while (1) {
        QueueSetMemberHandle_t active_queue = xQueueSelectFromSet(queue_set, portMAX_DELAY);

        if (active_queue == params->tcp_to_uart_queue) {
            Message msg;
            while (xQueueReceive(params->tcp_to_uart_queue, &msg, 0) == pdTRUE) {
                if (msg.hdr.kind == MESSAGE_KIND_DATA) {
                    uart_write_bytes(UART_PORT, msg.body, msg.hdr.len);
                } else if (msg.hdr.kind == MESSAGE_KIND_CONFIG) {
                    if (msg.hdr.len != sizeof(ConfigMessage)) {
                        ESP_LOGE(TAG, "invalid config message length: %d", msg.hdr.len);
                        continue;
                    }
                    ConfigMessage* config = (ConfigMessage*) msg.body;
                    apply_config(config);
                } else if (msg.hdr.kind == MESSAGE_KIND_CONTROL) {
                    if (msg.hdr.len != sizeof(ControlMessage)) {
                        ESP_LOGE(TAG, "invalid control message length: %d", msg.hdr.len);
                        continue;
                    }
                    ControlMessage* ctrl = (ControlMessage*) msg.body;
                    handle_control(ctrl, params);
                }
            }
        } else if (active_queue == uart_event_queue) {
            uart_event_t event;
            if (xQueueReceive(uart_event_queue, &event, 0) == pdTRUE) {
                if (event.type == UART_DATA) {
                    size_t buffered_len;
                    uart_get_buffered_data_len(UART_PORT, &buffered_len);

                    while (buffered_len > 0) {
                        Message msg;
                        msg.hdr.kind = MESSAGE_KIND_DATA;
                        size_t to_read = (buffered_len > MAX_MESSAGE_BODY_SIZE)
                                             ? MAX_MESSAGE_BODY_SIZE
                                             : buffered_len;

                        int len = uart_read_bytes(UART_PORT, msg.body, to_read, 0);
                        if (len > 0) {
                            msg.hdr.len = len;
                            xQueueSend(params->uart_to_tcp_queue, &msg, portMAX_DELAY);

                            uint64_t val = 1;
                            write(params->uart_to_tcp_efd, &val, sizeof(val));
                        }
                        uart_get_buffered_data_len(UART_PORT, &buffered_len);
                    }
                }
            }
        }
    }
}
