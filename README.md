# Setup the EchoKit server

You will need an EchoKit device, or create your own ESP32 device with the [EchoKit firmware](https://github.com/second-state/echokit_box).

## Build

```
git clone https://github.com/second-state/echokit_server
```

Edit `config.toml` to customize the VAD, ASR, LLM, TTS services, as well as prompts and MCP servers. You can [see many examples](examples/).

```
cargo build --release
```

## Configure AI services

The `config.toml` can use any combination of open-source or proprietary AI services, as long as they offer OpenAI-compatible API endpoints. Here are instructions to start open source AI servers for the EchoKit server.

* VAD: https://github.com/second-state/silero_vad_server
* ASR: https://llamaedge.com/docs/ai-models/speech-to-text/quick-start-whisper
* LLM: https://llamaedge.com/docs/ai-models/llm/quick-start-llm
* Streaming TTS: https://github.com/second-state/gsv_tts

Alternatively, you could use Google Gemini Live services for VAD + ASR + LLM, and even optionally, TTS. See [config.toml examples](examples/gemini).

You can also [configure MCP servers](examples/gaia/mcp/config.toml) to give the EchoKit server tool use capabilities. 

## Configure the device "skin"

The following two files in the server's current directory will be sent to the EchoKit device when it connects to the server.

* `background.gif` is the background image displayed on the device's screen.
* `hello.wav` is the greeting the device will say to prompt the user to speak.

## Run the EchoKit server

```
export RUST_LOG=debug
nohup target/release/echokit_server &
```

## Configure the device

Go to web page: https://echokit.dev/setup/  and use Bluetooth to connect to the `GAIA ESP332` device.

![Bluetooth connection](https://hackmd.io/_uploads/Hyjc9ZjEee.png)

Configure WiFi and server

* WiFi SSID (e.g., `MyHome`)
* WiFi password (e.g., `MyPassword`)
* Server URL (e.g., `ws://34.44.85.57:9090/ws/`) -- that IP address and port are for the server running `echokit_server`

![Configure Wifi](https://hackmd.io/_uploads/HJkh5ZjVee.png)

## Use the device

To start listening, press the `K0` button.

> Some devices do not have buttons, you should say trigger word `gaia` to start listening.

To reset wifi connection, press the `K2` button.





