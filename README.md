# tcpuart

Bridges a physical UART serial connection over Wi-Fi using an ESP32. The host side exposes the remote serial device as a local PTY (pseudo-terminal), so any application can use it as if it were a directly connected `/dev/ttyUSB0` or similar.

## Architecture

```
[ Serial Device ] <--UART--> [ ESP32 ] <--TCP/Wi-Fi--> [ Daemon ] <--PTY--> [ Your App ]
                                                             ^
                                                             |
                                                           [ CLI ]
```

There are two main parts to the project:

**ESP32 Firmware (`server/`)** — An ESP-IDF project written in C. It connects to Wi-Fi, opens a TCP server, and bridges all traffic bidirectionally between the TCP connection and a physical UART peripheral.

**Host Tools (Rust workspace)** — Three Cargo crates that run on the host machine (Linux):
- `daemon` — A background service that connects to the ESP32 over TCP and exposes the serial stream as a PTY device. Controlled via a Unix domain socket.
- `cli` — A command-line tool for managing the daemon (add/remove connections, list status, trigger remote reset).
- `common` — Shared protocol types and codecs used by the daemon and CLI.

## Requirements

- **ESP32-S3** (or any dual-core ESP32). Single-core variants (C3, S2) are not supported.
- **ESP-IDF v5.x** — [Installation guide](https://docs.espressif.com/projects/esp-idf/en/stable/esp32s3/get-started/index.html)
- **Rust** with a stable toolchain for building the host tools.

## Firmware

### Configuration

All configuration is done through `idf.py menuconfig` under the **TcpUart Server Configuration** menu.

| Option | Default | Description |
|---|---|---|
| TCP Port | `15113` | Port the ESP32 listens on for incoming connections |
| **Wi-Fi** | | |
| Number of networks | `1` | How many fallback Wi-Fi networks to configure (up to 5) |
| SSID / Password | — | Credentials for each configured network, tried in order |
| Max retries | `5` | Full cycles through all configured networks before rebooting |
| **UART** | | |
| Port number | `2` | UART peripheral to use (UART0 is reserved for console logs) |
| TX Pin | `17` | GPIO for UART TX |
| RX Pin | `16` | GPIO for UART RX |
| **Remote Reset** (optional) | | |
| Enable | `n` | Allow the host to trigger a hardware reset on the connected device |
| Reset GPIO | `18` | GPIO connected to the target device's reset pin |
| Active Low | `y` | Pull to GND to reset (uncheck for Active High) |
| Pulse Duration | `100ms` | How long the reset signal is held |
| **Status LED** (optional) | | |
| Enable | `n` | Use a WS2812B (NeoPixel) LED to indicate system status |
| GPIO | `48` | LED pin (48 is the built-in LED on the ESP32-S3-DevKitC-1) |
| Brightness | `20%` | Scale brightness down from the blinding default |
| UART Activity | `y` | Flash the LED on UART TX/RX data |

### Status LED behavior

If the status LED is enabled, it uses the following color scheme:

| Event | Color | Pattern |
|---|---|---|
| Connecting to Wi-Fi network | Yellow | Single flash |
| Wi-Fi connection failed (one network) | Red | Single flash |
| All Wi-Fi networks exhausted | Red | Solid for 3 seconds, then reboot |
| Wi-Fi connected (got IP) | Green | Single flash |
| TCP client connected | Blue | Single flash |
| TCP client disconnected | Blue | Single flash |
| Remote reset pin held | Purple | Solid while held |
| UART TX data | Red | Brief flash (50ms) |
| UART RX data | Green | Brief flash (50ms) |

### Building and flashing

```sh
cd server
idf.py set-target esp32s3
idf.py menuconfig   # configure Wi-Fi credentials, UART pins, etc.
idf.py build
idf.py flash monitor
```

## Host Tools

### Host Tools

Install both binaries directly from the repository root:

```sh
cargo install --path cli
cargo install --path daemon
```

This compiles both in release mode and places `tcpuart` and `tcpuart-daemon` in `~/.cargo/bin/`, which is already on your `$PATH` if you have a standard Rust installation.

### Daemon

The daemon manages one or more named connections to ESP32 devices. Each connection gets its own PTY device on the host. It listens on a Unix domain socket for CLI commands.

```sh
tcpuart-daemon
```

The socket path defaults to `/tmp/tcpuart.sock` and can be overridden with the `--socket` flag or the `TCPUART_SOCKET` environment variable. The `RUST_LOG` environment variable controls log output (e.g. `RUST_LOG=info`).

### CLI

**Commands:**

```sh
# Add a connection (creates a PTY, prints the device path e.g. /dev/pts/3)
tcpuart add <name> <esp32-ip> [port]

# List all active connections and their PTY paths
tcpuart list

# Remove a connection
tcpuart remove <name>

# Trigger a hardware reset on the connected device (requires firmware support)
tcpuart reset <name>
```

The CLI uses the same `--socket` / `TCPUART_SOCKET` default as the daemon. If you override the socket path on the daemon, set the same value in the environment for the CLI or pass `--socket` explicitly.

### Running as a background service

#### Linux (systemd user service)

Create `~/.config/systemd/user/tcpuart.service`:

```ini
[Unit]
Description=TCP UART Bridge Daemon
After=network.target

[Service]
Type=simple
ExecStart=%h/.cargo/bin/tcpuart-daemon
Restart=on-failure
Environment=RUST_LOG=info

[Install]
WantedBy=default.target
```

Then enable and start it:

```sh
systemctl --user daemon-reload
systemctl --user enable --now tcpuart
```

#### macOS (LaunchAgent)

Create `~/Library/LaunchAgents/com.tcpuart.daemon.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.tcpuart.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Users/YOUR_USERNAME/.cargo/bin/tcpuart-daemon</string>
    </array>
    <key>EnvironmentVariables</key>
    <dict>
        <key>RUST_LOG</key>
        <string>info</string>
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>
```

Then load it:

```sh
launchctl load ~/Library/LaunchAgents/com.tcpuart.daemon.plist
```

> **Note:** launchd does not expand `~` or `$HOME` in plist files, so you must substitute your actual username in the `ProgramArguments` path.

## Wi-Fi fallback behavior

The firmware tries each configured network in order. If a connection drops or fails, it cycles through all networks before incrementing the retry counter. Once the retry counter reaches the configured maximum, the ESP32 logs the failure and reboots after a short delay to attempt a clean recovery.
