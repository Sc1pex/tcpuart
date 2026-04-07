#include "tcp.h"
#include "uart.h"
#include "wifi.h"

void app_main(void) {
    wifi_init();
    start_uart_task();
    start_tcp_task();
}
