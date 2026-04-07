#include "tcp.h"
#include "wifi.h"

void app_main(void) {
    wifi_init();
    start_tcp_task();
}
