#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "tcp.h"
#include "uart.h"
#include "wifi.h"

void app_main(void) {
    wifi_init();

    xTaskCreatePinnedToCore(uart_task, "uart_task", 4096, NULL, 5, NULL, 0);
    xTaskCreatePinnedToCore(tcp_task, "tcp_task", 4096, NULL, 5, NULL, 0);
}
