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
MAC address:       b4:3a:45:a5:08:14
App/part. size:    2,359,792/8,323,072 bytes, 28.35%
[00:00:00] [========================================]      14/14      0x0                                             [00:00:00] [========================================]       1/1       0x8000                                          [00:00:19] [========================================]    1608/1608    0x10000                                         [2025-04-28T16:52:24Z INFO ] Flashing has completed!
Commands:
    CTRL+R    Reset chip
    CTRL+C    Exit

ESP-ROM:esp32s3-20210327
Build:Mar 27 2021
rst:0x15 (USB_UART_CHIP_RESET),boot:0x8 (SPI_FAST_FLASH_BOOT)
Saved PC:0x40379f5e
0x40379f5e - wdev_dump_rx_linked_list
    at ??:??
SPIWP:0xee
mode:DIO, clock div:2
load:0x3fce3818,len:0x16f8
load:0x403c9700,len:0x4
load:0x403c9704,len:0xc00
load:0x403cc700,len:0x2eb0
entry 0x403c9908
I (27) boot: ESP-IDF v5.1-beta1-378-gea5e0ff298-dirt 2nd stage bootloader
I (27) boot: compile time Jun  7 2023 08:07:32
I (28) boot: Multicore bootloader
I (33) boot: chip revision: v0.2
I (36) boot.esp32s3: Boot SPI Speed : 40MHz
I (41) boot.esp32s3: SPI Mode       : DIO
I (46) boot.esp32s3: SPI Flash Size : 8MB
I (51) boot: Enabling RNG early entropy source...
I (56) boot: Partition Table:
I (60) boot: ## Label            Usage          Type ST Offset   Length
I (67) boot:  0 nvs              WiFi data        01 02 00009000 00006000
I (74) boot:  1 phy_init         RF data          01 01 0000f000 00001000
I (82) boot:  2 factory          factory app      00 00 00010000 007f0000
I (89) boot: End of partition table
I (94) esp_image: segment 0: paddr=00010020 vaddr=3c120020 size=1115f8h (1119736) map
I (380) esp_image: segment 1: paddr=00121620 vaddr=3fc9ac00 size=06eb0h ( 28336) load
I (389) esp_image: segment 2: paddr=001284d8 vaddr=40374000 size=07b40h ( 31552) load
I (398) esp_image: segment 3: paddr=00130020 vaddr=42000020 size=1110f8h (1118456) map
I (676) esp_image: segment 4: paddr=00241120 vaddr=4037bb40 size=0f0a0h ( 61600) load
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

![Bluetooth connection](https://hackmd.io/_uploads/HkcXIqVmxe.png)

Configure WiFi and server

* WiFi SSID (e.g., `MyHome`)
* WiFi password (e.g., `MyPassword`)
* Server URL (e.g., `ws://34.44.85.57:9090/ws/`) -- that IP address and port are for the server running `esp_assistant`

![Configure Wifi](https://hackmd.io/_uploads/HkOqLq4Qge.png)

## Use the device

To start listening, press the `K0` button.

> Some devices do not have buttons, you should say trigger word `gaia` to start listening.

To reset wifi connection, press the `K2` button.





