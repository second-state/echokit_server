# Setup the EchoKit server

EchoKit Server is the central component that manages communication between the EchoKit device and AI services. It can be deployed locally or connected to preset servers, allowing developers to customize LLM endpoints, plan the LLM prompt, configure speech models, and integrate additional AI features like MCP servers.

<br>
<div align="center">
    <a href="https://echokit.dev/">Website</a> |
    <a href="https://discord.gg/Fwe3zsT5g3">Discord</a> |
    <a href="https://youtu.be/Zy-rLT4EgZQ">Live Demo</a> |
    <a href="https://echokit.dev/docs/quick-start/">Documentation</a>
</div>
</br>

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

## Configure the voice prompt

The `hello.wav` file on the server is sent to the EchoKit device when it connects. It is the voice prompt the device will say to tell the user that it is ready.

## Run the EchoKit server

```
export RUST_LOG=debug
nohup target/release/echokit_server &
```

## Test on a web page

Go here: https://echokit.dev/chat/

Click on the link to save the `index.html` file to your local hard disk.

Double click the local `index.html` file and open it in your browser. 

In the web page, set the URL to your own EchoKit server address, and start chatting!

## Configure a new device

Go to web page: https://echokit.dev/setup/  and use Bluetooth to connect to the `GAIA ESP332` device.

![Bluetooth connection](https://hackmd.io/_uploads/Hyjc9ZjEee.png)

Configure WiFi and server

* WiFi SSID (e.g., `MyHome`)
* WiFi password (e.g., `MyPassword`)
* Web Socket server URL for `echokit_server`
    * US: `ws://indie.echokit.dev/ws/`
    * Asia: `ws://hk.echokit.dev/ws/`

![Configure Wifi](https://hackmd.io/_uploads/HJkh5ZjVee.png)

## Use the device

**Chat:** press the `K0` button once or multiple times util the screen shows "Listening ...". You can now speak and it will answer.

**Record:** long press the `K0` until the screen shows "Recording ...". You can now speak and the audio will be recorded on the server.

**Config:** press `RST`. While it is restarting, press and hold `K0` to enter the configuration mode. Then [open the configuration UI](https://echokit.dev/setup/) to connect to the device via BT.
