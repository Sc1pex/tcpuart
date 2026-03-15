# tcpuart

A Linux TTY driver that bridges virtual serial devices to remote ESP8266 units over TCP. This project allows you to access remote hardware UARTs as if they were local `/dev/tcpuartX` devices.

The system consists of three main components:
1. **Kernel Module**: A Linux driver that implements the TTY interface and manages the TCP client logic.
2. **Manager**: A Rust-based CLI tool used to request new connections and monitor existing ones.
3. **Server**: Firmware for ESP8266 (and compatible) devices that acts as a TCP-to-UART bridge.

## Build & Installation

### 1. Kernel Module
**Prerequisites**: Linux kernel headers, `gcc`, `make`.

1. Build the module:
   ```bash
   cd kernel_module
   make
   ```
2. Load the module:
   ```bash
   sudo insmod tcpuart.ko
   ```
   *Note: This creates the control device at `/dev/tcpuart0`.*

### 2. Manager CLI
**Prerequisites**: Rust toolchain (`cargo`).

1. Build the manager:
   ```bash
   cd manager
   cargo build --release
   ```
2. The binary will be available at `target/release/tcpuart-manager`.

### 3. Server Firmware
**Prerequisites**: PlatformIO.

1. Configure your WiFi credentials:
   ```bash
   cd server
   cp .env.example .env
   # Edit .env with your WIFI_SSID and WIFI_PASSWORD
   ```
2. Build and upload the firmware:
   ```bash
   pio run -t upload
   ```

## Usage

Once the kernel module is loaded and the server is running on your ESP8266:

1. **Connect to a remote server**:
   ```bash
   # Replace <IP> with the ESP8266 IP address
   ./tcpuart-manager connect <IP> 15113
   ```
   This will output the assigned device name, e.g., `Created device /dev/tcpuart1`.

2. **Access the TTY**:
   You can now use any serial terminal (minicom, screen, picocom) on the new device:
   ```bash
   screen /dev/tcpuart1 115200
   ```
   *Note: Baud rate and parity changes in your terminal are automatically synced to the remote ESP8266 UART.*

3. **Manage connections**:
   ```bash
   # List active devices
   ./tcpuart-manager list

   # Show status of a specific device
   ./tcpuart-manager status 1

   # Destroy a connection (minor number 1)
   ./tcpuart-manager destroy 1
   ```
