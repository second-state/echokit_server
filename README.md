# EchoKit Server

EchoKit Server is the central component that manages communication between the [EchoKit device](https://echokit.dev/) and AI services. It can be deployed locally or connected to preset servers, allowing developers to customize LLM endpoints, plan the LLM prompt, configure speech models, and integrate additional AI features like MCP servers.

<br>
<div align="center">
    <a href="https://echokit.dev/">Website</a> |
    <a href="https://discord.gg/Fwe3zsT5g3">Discord</a> |
    <a href="https://youtu.be/Zy-rLT4EgZQ">Live Demo</a> |
    <a href="https://echokit.dev/docs/quick-start/">Documentation</a>
</div>
</br>

You will need an [EchoKit device](https://echokit.dev/), or create your own ESP32 device with the [EchoKit firmware](https://github.com/second-state/echokit_box).


## Features

EchoKit Server powers the full voice–AI interaction loop, making it easy for developers to run end-to-end speech pipelines with flexible model choices and custom integrations.

### ASR → LLM → TTS Pipeline

Seamlessly connect **ASR → LLM → TTS** for real-time, natural conversations.
Each stage can be configured independently with your preferred models or APIs.

#### Model Compatibility

* **ASR (Speech Recognition):** Works with any API that’s *OpenAI-compatible*.
* **LLM (Language Model):** Connect to any *OpenAI-spec* endpoint — local or cloud.
* **TTS (Text-to-Speech):** Use any *OpenAI-spec* voice model for flexible deployment.
    * ElevenLabs (Streaming Mode)

### End-to-End Model Pipelines

Out-of-the-box support for:

* **Gemini** — Google’s multimodal model
* **Qwen Real-Time** — Alibaba’s powerful open LLM

### Developer Customization

* Deploy **locally** or connect to **remote inference servers**
* Define your own **LLM prompts** and **response workflows**
* Configure **speech and voice models** for different personas or use cases
* Integrate **MCP servers** for extended functionality


## Set up the EchoKit server

### Build

```
git clone https://github.com/second-state/echokit_server
```

Edit `config.toml` to customize the VAD, ASR, LLM, TTS services, as well as prompts and MCP servers. You can [see many examples](examples/).

```
cargo build --release
```

**Note for aarch64 (ARM64) builds:** When cross-compiling for aarch64, the required `fp16` target feature is automatically enabled via `.cargo/config.toml`. If building natively on aarch64, you may need to set:

```
RUSTFLAGS="-C target-feature=+fp16" cargo build --release
```

### Configure AI services

The `config.toml` can use any combination of open-source or proprietary AI services, as long as they offer OpenAI-compatible API endpoints. Here are instructions to start open source AI servers for the EchoKit server.

* ASR: https://llamaedge.com/docs/ai-models/speech-to-text/quick-start-whisper
* LLM: https://llamaedge.com/docs/ai-models/llm/quick-start-llm
* Streaming TTS: https://github.com/second-state/gsv_tts

Alternatively, you could use Google Gemini Live services for VAD + ASR + LLM, and even optionally, TTS. See [config.toml examples](examples/gemini).

You can also [configure MCP servers](examples/mcp/config.toml) to give the EchoKit server tool use capabilities. 

### Configure the voice prompt

The `hello.wav` file on the server is sent to the EchoKit device when it connects. It is the voice prompt the device will say to tell the user that it is ready.

### Run the EchoKit server

```
export RUST_LOG=debug
nohup target/release/echokit_server &
```

### Test on a web page

Go here: https://echokit.dev/chat/

Click on the link to save the `index.html` file to your local hard disk.

Double click the local `index.html` file and open it in your browser. 

In the web page, set the URL to your own EchoKit server address, and start chatting!

### Configure a new device

Go to web page: https://echokit.dev/setup/  and use Bluetooth to connect to the `GAIA ESP332` device.

![Bluetooth connection](https://hackmd.io/_uploads/Hyjc9ZjEee.png)

Configure WiFi and server

* WiFi SSID (e.g., `MyHome`)
* WiFi password (e.g., `MyPassword`)
* Web Socket server URL for `echokit_server`
    * US: `ws://indie.echokit.dev/ws/`
    * Taiwan: `ws://tw.echokit.dev/ws/`
    * Rest of the world: `ws://edge.echokit.dev/ws/`

![Configure Wifi](https://hackmd.io/_uploads/HJkh5ZjVee.png)

### Use the device

**Chat:** press the `K0` button once or multiple times until the status bar shows "Ready". You can now speak and it will show "Listening ...". The device answers after it decides that you have done speaking.

**Config:** press `RST`. While it is restarting, press and hold `K0` to enter the configuration mode. Then [open the configuration UI](https://echokit.dev/setup/) to connect to the device via BT.
