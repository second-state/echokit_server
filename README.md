# Setup the EchoKit device and server

## Build espflash

Assume that you [installed the Rust compiler](https://www.rust-lang.org/tools/install) on your computer.

```
cargo install cargo-espflash espflash ldproxy
```

## Get firmware

Get a pre-compiled binary version of the firmware.

```
curl -LO https://echokit.dev/firmware/esp32-s3-box-hello
```

## Upload firmware

You MUST connect the computer to the SLAVE USB port on the device. Allow the computer to accept connection from the device. The detected USB serial port must be `JTAG`. IT CANNOT be `USB Single`.

```
$ espflash flash --monitor --flash-size 16mb esp32-s3-box-hello
```

The response is as follows.

```
[2025-04-28T16:51:43Z INFO ] Detected 2 serial ports
[2025-04-28T16:51:43Z INFO ] Ports which match a known common dev board are highlighted
[2025-04-28T16:51:43Z INFO ] Please select a port
✔ Remember this serial port for future use? · no
[2025-04-28T16:52:00Z INFO ] Serial port: '/dev/cu.usbmodem2101'
[2025-04-28T16:52:00Z INFO ] Connecting...
[2025-04-28T16:52:00Z INFO ] Using flash stub
Chip type:         esp32s3 (revision v0.2)
Crystal frequency: 40 MHz
Flash size:        8MB
Features:          WiFi, BLE
... ...
I (705) boot: Loaded app from partition at offset 0x10000
I (705) boot: Disabling RNG early entropy source...
I (716) cpu_start: Multicore app
```

## Reset the device

Reset the device (simulate the RST button or power up).

```
$ espflash reset
```

Delete the existing firmware if needed.

```
$ espflash erase-flash
```

## Set up the EchoKit server

### Build

```
$ git clone https://github.com/second-state/esp_assistant
```

Edit `config.toml` to customize the ASR, LLM, TTS services, as well as prompts and MCP servers. You can [see many examples](examples/).

```
$ cargo build --release
```

### Start

```
$ export RUST_LOG=debug
$ nohup target/release/esp_assistant &
```

## Configure the device

Go to web page: https://echokit.dev/setup/  and use Bluetooth to connect to the `GAIA ESP332` device.

![Bluetooth connection](https://hackmd.io/_uploads/Hyjc9ZjEee.png)

Configure WiFi and server

* WiFi SSID (e.g., `MyHome`)
* WiFi password (e.g., `MyPassword`)
* Server URL (e.g., `ws://34.44.85.57:9090/ws/`) -- that IP address and port are for the server running `esp_assistant`

![Configure Wifi](https://hackmd.io/_uploads/HJkh5ZjVee.png)

## Use the device

To start listening, press the `K0` button.

> Some devices do not have buttons, you should say trigger word `gaia` to start listening.

To reset wifi connection, press the `K2` button.





