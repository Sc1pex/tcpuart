#pragma once

#include <stdint.h>

#define MAX_MESSAGE_BODY_SIZE 255

typedef enum {
    MESSAGE_KIND_DATA = 1,
    MESSAGE_KIND_CONFIG = 2,
    MESSAGE_KIND_CONTROL = 3,
    MESSAGE_KIND_RESPONSE = 4,
} MessageKind;

typedef struct __attribute__((packed)) {
    uint8_t kind;
    uint8_t len;
} Header;

typedef struct __attribute__((packed)) {
    Header hdr;
    uint8_t body[MAX_MESSAGE_BODY_SIZE];
} Message;

typedef struct __attribute__((packed)) {
    uint32_t baud_rate;
    uint8_t data_bits;
    uint8_t stop_bits;
    uint8_t parity;
} ConfigMessage;

typedef struct __attribute__((packed)) {
    uint8_t command;
} ControlMessage;

typedef struct __attribute__((packed)) {
    uint8_t status;
} ResponseMessage;

#define CONTROL_CMD_RESET 1

#define RESPONSE_STATUS_OK 1
#define RESPONSE_STATUS_NOT_SUPPORTED 2

