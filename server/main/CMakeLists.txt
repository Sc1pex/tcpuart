set(srcs "main.c" "wifi.c" "tcp.c" "uart.c")

if(CONFIG_ESP_STATUS_LED_ENABLED)
    list(APPEND srcs "status.c")
endif()

idf_component_register(SRCS ${srcs}
                    INCLUDE_DIRS ".")
