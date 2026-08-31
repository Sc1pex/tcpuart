#include "wifi.h"
#include <stddef.h>
#include "esp_log.h"
#include "esp_netif_types.h"
#include "esp_wifi.h"
#include "esp_wifi_types_generic.h"
#include "freertos/FreeRTOS.h"
#include "freertos/event_groups.h"
#include "nvs_flash.h"
#include "sdkconfig.h"

#define MAX_RETRIES CONFIG_ESP_WIFI_MAX_RETRIES

typedef struct {
    uint8_t ssid[32];
    uint8_t password[64];
} wifi_cred_t;

static const wifi_cred_t s_wifi_networks[] = {
    // Network 1 is always present
    { CONFIG_ESP_WIFI_SSID_1, CONFIG_ESP_WIFI_PASSWORD_1 },

// Conditionally compile the rest based on Kconfig definitions
#ifdef CONFIG_ESP_WIFI_SSID_2
    { CONFIG_ESP_WIFI_SSID_2, CONFIG_ESP_WIFI_PASSWORD_2 },
#endif

#ifdef CONFIG_ESP_WIFI_SSID_3
    { CONFIG_ESP_WIFI_SSID_3, CONFIG_ESP_WIFI_PASSWORD_3 },
#endif

#ifdef CONFIG_ESP_WIFI_SSID_4
    { CONFIG_ESP_WIFI_SSID_4, CONFIG_ESP_WIFI_PASSWORD_4 },
#endif

#ifdef CONFIG_ESP_WIFI_SSID_5
    { CONFIG_ESP_WIFI_SSID_5, CONFIG_ESP_WIFI_PASSWORD_5 },
#endif
};
static const int s_num_wifi_networks = sizeof(s_wifi_networks) / sizeof(s_wifi_networks[0]);

static const char* TAG = "wifi";
static EventGroupHandle_t wifi_events;
#define CONNECTED_BIT BIT0
#define FAILED_BIT BIT1

static int s_retry_count = 0;
static int s_current_wifi_idx = 0;
static wifi_config_t s_current_config;

static void update_config_and_connect() {
    memcpy(
        s_current_config.sta.ssid, s_wifi_networks[s_current_wifi_idx].ssid,
        sizeof(s_wifi_networks[s_current_wifi_idx].ssid)
    );
    memcpy(
        s_current_config.sta.password, s_wifi_networks[s_current_wifi_idx].password,
        sizeof(s_wifi_networks[s_current_wifi_idx].password)
    );

    ESP_LOGI(TAG, "trying to connect to %.32s...", s_current_config.sta.ssid);

    ESP_ERROR_CHECK(esp_wifi_set_config(WIFI_IF_STA, &s_current_config));
    esp_wifi_connect();
}

static void event_handler(void* arg, esp_event_base_t base, int32_t event_id, void* event_data) {
    if (base == WIFI_EVENT && event_id == WIFI_EVENT_STA_START) {
        update_config_and_connect();
    } else if (base == WIFI_EVENT && event_id == WIFI_EVENT_STA_DISCONNECTED) {
        s_current_wifi_idx++;
        if (s_current_wifi_idx >= s_num_wifi_networks) {
            s_current_wifi_idx = 0;

            s_retry_count++;
            if (s_retry_count == MAX_RETRIES) {
                xEventGroupSetBits(wifi_events, FAILED_BIT);
                return;
            }

            ESP_LOGW(
                TAG, "tried connecting to all networks. retrying (%d/%d)", s_retry_count,
                MAX_RETRIES
            );
        }

        update_config_and_connect();

    } else if (base == IP_EVENT && event_id == IP_EVENT_STA_GOT_IP) {
        ip_event_got_ip_t* event = (ip_event_got_ip_t*) event_data;
        ESP_LOGI(TAG, "got IP: " IPSTR, IP2STR(&event->ip_info.ip));
        s_retry_count = 0;
        xEventGroupSetBits(wifi_events, CONNECTED_BIT);
    }
}

void wifi_init() {
    esp_err_t ret = nvs_flash_init();
    if (ret == ESP_ERR_NVS_NO_FREE_PAGES || ret == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        ESP_ERROR_CHECK(nvs_flash_erase());
        ret = nvs_flash_init();
    }
    ESP_ERROR_CHECK(ret);

    wifi_events = xEventGroupCreate();

    ESP_ERROR_CHECK(esp_netif_init());
    ESP_ERROR_CHECK(esp_event_loop_create_default());
    esp_netif_create_default_wifi_sta();

    wifi_init_config_t cfg = WIFI_INIT_CONFIG_DEFAULT();
    ESP_ERROR_CHECK(esp_wifi_init(&cfg));

    ESP_ERROR_CHECK(esp_event_handler_register(WIFI_EVENT, ESP_EVENT_ANY_ID, &event_handler, NULL));
    ESP_ERROR_CHECK(
        esp_event_handler_register(IP_EVENT, IP_EVENT_STA_GOT_IP, &event_handler, NULL)
    );

    ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_STA));
    ESP_ERROR_CHECK(esp_wifi_start());
    ESP_ERROR_CHECK(esp_wifi_set_ps(WIFI_PS_NONE));

    EventBits_t bits = xEventGroupWaitBits(
        wifi_events, CONNECTED_BIT | FAILED_BIT, pdFALSE, pdFALSE, portMAX_DELAY
    );

    if (bits & CONNECTED_BIT) {
        ESP_LOGI(TAG, "connected");
    } else {
        ESP_LOGE(TAG, "failed to connect to any network. Rebooting...");
        vTaskDelay(pdMS_TO_TICKS(5000));
        esp_restart();
    }
}
