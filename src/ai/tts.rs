use std::collections::LinkedList;

use bytes::Bytes;

// LLM 真是绝了
impl crate::config::TTSTextOptimizationConfig {
    pub(crate) fn default_prompt() -> LinkedList<super::llm::Content> {
        match std::env::var("TTS_OPTIMIZATION_PROMPT_LANG")
            .unwrap_or_else(|_| "en".to_string())
            .to_lowercase()
            .as_str()
        {
            "en" => Self::default_prompt_en(),
            _ => Self::default_prompt_zh(),
        }
    }

    pub(crate) fn default_prompt_en() -> LinkedList<super::llm::Content> {
        let system_prompt= r#"
        Role
        You are a professional TTS (Text-to-Speech) text preprocessing expert. Your task is to optimize the raw text input from the user into the "broadcast text" that is most suitable for speech synthesis engines.
        
        Goal
        The input text may contain numbers, symbols, English abbreviations, dates, URLs, etc.
        that are difficult to read directly. You need to convert them into natural language that conforms to human oral habits, ensuring that the TTS reads it fluently, accurately, and with emotion, avoiding mechanical reading or misreading.

        Constraints & Rules
        Language Consistency:
        English Context: Keep English, but convert numbers to their full English word forms (e.g., 123 -> one hundred twenty-three).
        Number and Date Reading Handling (Core):
        Ordinals/Numbering:
        English: e.g., "No.1", "1st" -> read as "Number one", "first".
        Years (Key Optimization):
        English (Strictly follow oral habits):
        Ordinary Years (1100-1999): Split into two two-digit numbers.
        "1985" -> "nineteen eighty-five" (prohibit reading as one thousand nine hundred...).
        "1492" -> "fourteen ninety-two".
        "1100" -> "eleven hundred".
        Whole Hundred Years:
        "1900" -> "nineteen hundred".
        Year 2000 and beyond:
        "2000" -> "two thousand".
        "2001" -> "two thousand and one".
        "2024" -> "twenty twenty-four" (recommended) or "two thousand and twenty-four".
        Future Years:
        "2100" -> "twenty-one hundred".
        Dates:
        English: "May 1st, 2024" -> "May first, twenty twenty-four".
        English: "12/25/2024" -> "December twenty-fifth, twenty twenty-four".
        Time:
        English: "16:30" -> "four thirty PM" or "sixteen thirty".
        Phone Numbers/IDs: Keep reading digit by digit.
        English: "555-0199" -> "five five five, zero one nine nine".
        Decimals/Percentages:
        English: "3.14" -> "three point one four", "50%" -> "fifty percent".
        Amounts:
        English: Read out the unit (e.g., "$100" -> "one hundred dollars").
        Punctuation and Symbol Optimization:
        Convert difficult-to-read symbols into natural pauses or conjunctions.
        "/" (Slash): Depending on the context, convert to "or", "per", "and", or directly use a pause.
        "&" (And Symbol): Read as "and" (English).
        "@": Read as "at" (English).
        "\\#": Read as "hashtag" / "number sign" (English).
        Special Format Handling:
        URL/Links: Do not attempt to read out complex https://..., usually replace with "link address" or delete it directly, unless it is a very short domain name.
        Code/Commands: If it is a simple command, try to oralize it (e.g., Ctrl+C -> "press and hold the Control key and then press C").
        Output Requirements:
        Only output the converted clean text.
        Do not include any explanations, do not add quotes, do not use Markdown formatting.
        "#.to_string();
        let mut prompts = LinkedList::new();
        prompts.push_back(super::llm::Content {
            role: super::llm::Role::System,
            message: system_prompt,
            tool_calls: None,
            tool_call_id: None,
        });
        prompts.push_back(super::llm::Content {
            role: super::llm::Role::User,
            message: "2026 year is a year full of hope.".to_string(),
            tool_calls: None,
            tool_call_id: None,
        });
        prompts.push_back(super::llm::Content {
            role: super::llm::Role::Assistant,
            message: "twenty twenty-six year is a year full of hope.".to_string(),
            tool_calls: None,
            tool_call_id: None,
        });
        prompts
    }

    pub(crate) fn default_prompt_zh() -> LinkedList<super::llm::Content> {
        let system_prompt=  r#"Role
你是一个专业的 TTS（文本转语音）文本预处理专家。你的任务是将用户输入的原始文本优化为最适合语音合成引擎朗读的“口播文本”。

Goal
输入的文本可能包含数字、符号、英文缩写、日期、URL 等难以直接朗读的内容。你需要将它们转换为符合人类口语习惯的自然语言，确保 TTS 读出来流畅、准确、有情感，避免出现机械朗读或读错的情况。

Constraints & Rules
语言一致性：
中文语境：将所有阿拉伯数字转换为对应的中文汉字（如 1 -> 一，2023 -> 二零二三）。
英文语境：保持英文，但将数字转换为完整的英文单词形式（如 123 -> one hundred twenty-three）。
数字与日期读法处理（核心）：
序号/编号：
中文：如 “第1名”、“No.1”、“1楼” -> 读作 “第一名”、“第一号”、“一楼”。
英文：如 “No.1”、“1st” -> 读作 “Number one”、“first”。
年份（重点优化）：
中文：
“1995” -> “一九九五年” 或 “一千九百九十五年”（推荐前者更自然）。
“2024” -> “二零二四年”。
英文（严格遵循口语习惯）：
普通年份 (1100-1999)：拆分为两个两位数。
“1985” -> “nineteen eighty-five” (禁止读作 one thousand nine hundred…)。
“1492” -> “fourteen ninety-two”。
“1100” -> “eleven hundred”。
整百年份：
“1900” -> “nineteen hundred”。
2000年及以后:
“2000” -> “two thousand”。
“2001” -> “two thousand and one”。
“2024” -> “twenty twenty-four” (推荐) 或 “two thousand and twenty-four”。
未来年份：
“2100” -> “twenty-one hundred”。
日期：
中文：“2023-12-13” -> “二零二三年十二月十三日”。
英文：“May 1st, 2024” -> “May first, twenty twenty-four”。
英文：“12/25/2024” -> “December twenty-fifth, twenty twenty-four”。
时间：
中文：“16:30” -> “下午四点半” 或 “十六点三十分”。
英文：“16:30” -> “four thirty PM” 或 “sixteen thirty”。
电话号码/ID：保持逐位朗读。
中文：“13812345678” -> “一三八一二三四五六七八”。
英文：“555-0199” -> “five five five, zero one nine nine”。
小数/百分比：
中文：“3.14” -> “三点一四”，“50%” -> “百分之五十”。
英文：“3.14” -> “three point one four”，“50%” -> “fifty percent”。
金额：
中文：如有货币符号，读出单位（如 “$100” -> “一百美元”，“¥9.9” -> “九块九”）。
英文：读出单位（如 “$100” -> “one hundred dollars”）。
标点与符号优化：
将难以朗读的符号转换为自然的停顿或连接词。
“/” (斜杠)：根据语境转换为 “或”、“每”、“以及”，或者直接用顿号停顿。
“&” (and 符号)：中文读 “和”，英文读 “and”。
“@”：读作 “艾特” (中文) 或 “at” (英文)。
“#”: 读作 “井号” (中文) 或 “hashtag” / “number sign” (英文)。
特殊格式处理：
URL/链接：不要尝试读出复杂的 https://...，通常替换为 “链接地址” 或直接删除，除非是极短的域名。
代码/命令：如果是简单的命令，尝试口语化（如 Ctrl+C -> “按住 Control 键再按 C”）。
输出要求：
只输出转换后的纯净文本。
不要包含任何解释、不要加引号、不要 Markdown 标记
"#.to_string();
        let mut prompts = LinkedList::new();
        prompts.push_back(super::llm::Content {
            role: super::llm::Role::System,
            message: system_prompt,
            tool_calls: None,
            tool_call_id: None,
        });
        prompts.push_back(super::llm::Content {
            role: super::llm::Role::User,
            message: "2026年是一个充满希望的年份。".to_string(),
            tool_calls: None,
            tool_call_id: None,
        });
        prompts.push_back(super::llm::Content {
            role: super::llm::Role::Assistant,
            message: "二零二六年是一个充满希望的年份。".to_string(),
            tool_calls: None,
            tool_call_id: None,
        });
        prompts.push_back(super::llm::Content {
            role: super::llm::Role::User,
            message: "2026 year is a year full of hope.".to_string(),
            tool_calls: None,
            tool_call_id: None,
        });
        prompts.push_back(super::llm::Content {
            role: super::llm::Role::Assistant,
            message: "twenty twenty-six year is a year full of hope.".to_string(),
            tool_calls: None,
            tool_call_id: None,
        });
        prompts
    }
}

/// return: wav_audio: 16bit,32k,single-channel.
pub async fn gsv(
    client: &reqwest::Client,
    tts_url: &str,
    speaker: &str,
    text: &str,
    sample_rate: Option<usize>,
    llm_voice_opt: Option<&crate::config::TTSTextOptimizationConfig>,
) -> anyhow::Result<Bytes> {
    log::debug!("speaker: {speaker}, text: {text}");
    let mut text = text.to_string();
    if let Some(opt) = llm_voice_opt {
        let mut optimized_text_response = crate::ai::llm_stable(
            &opt.url,
            &opt.api_key,
            &opt.model,
            None,
            opt.prompts
                .iter()
                .chain(std::iter::once(&crate::ai::llm::Content {
                    role: crate::ai::llm::Role::User,
                    message: text.to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                })),
            Vec::new(),
        )
        .await?;
        let mut optimized_text = String::new();
        while let Ok(chunk) = optimized_text_response.next_chunk().await {
            match chunk {
                crate::ai::StableLLMResponseChunk::Functions(..) => {
                    #[cfg(debug_assertions)]
                    unreachable!("TTS text optimization should not return function calls");
                }
                crate::ai::StableLLMResponseChunk::Text(chunk) => {
                    optimized_text.push_str(&chunk);
                }
                crate::ai::StableLLMResponseChunk::Stop => {
                    break;
                }
            }
        }
        log::debug!("optimized text: {}", optimized_text);
        text = optimized_text;
    }
    let res = client
        .post(tts_url)
        .json(&serde_json::json!({"speaker": speaker, "input": text, "sample_rate": sample_rate}))
        // .body(serde_json::json!({"speaker": speaker, "input": text}).to_string())
        .send()
        .await?;
    let status = res.status();
    if status != 200 {
        let body = res.text().await?;
        return Err(anyhow::anyhow!(
            "tts failed, status:{status}, body:{}",
            body
        ));
    }
    let bytes = res.bytes().await?;
    log::info!("TTS response: {:?}", bytes.len());
    Ok(bytes)
}

// cargo test --package esp_assistant --bin esp_assistant -- ai::tts::test_gsv --exact --show-output
#[tokio::test]
async fn test_gsv() {
    let tts_url = "http://localhost:8000/v1/audio/speech";
    let speaker = "ad";
    let text = "你好，我是胡桃";
    let client = reqwest::Client::new();
    let wav_audio = gsv(&client, tts_url, speaker, text, Some(16000), None)
        .await
        .unwrap();
    let header = hound::WavReader::new(wav_audio.as_ref()).unwrap();
    let spec = header.spec();
    println!("wav header: {:?}", spec);
    assert_eq!(spec.sample_rate, 16000);
    std::fs::write("./resources/test/out.wav", wav_audio).unwrap();
}

// cargo test --package esp_assistant --bin esp_assistant -- ai::tts::test_gsv_with_opt --exact --show-output
#[tokio::test]
async fn test_gsv_with_opt() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    let tts_url = std::env::var("GSV_TTS_URL").unwrap();
    let tts_speaker = std::env::var("GSV_TTS_SPEAKER").unwrap_or_else(|_| "ad".to_string());

    let llm_voice_opt = crate::config::TTSTextOptimizationConfig {
        url: std::env::var("GSV_TTS_OPT_LLM_URL").unwrap(),
        api_key: std::env::var("GSV_TTS_OPT_LLM_API_KEY").unwrap(),
        model: std::env::var("GSV_TTS_OPT_LLM_MODEL")
            .unwrap_or_else(|_| "openai/gpt-4o-mini".to_string()),
        prompts: crate::config::TTSTextOptimizationConfig::default_prompt(),
    };

    let text = "2026年是一个充满希望的年份，欢迎访问https://example.com获取更多信息。";
    let client = reqwest::Client::new();

    let now = std::time::Instant::now();
    let wav_audio = gsv(
        &client,
        &tts_url,
        &tts_speaker,
        text,
        Some(16000),
        Some(&llm_voice_opt),
    )
    .await
    .unwrap();
    println!(
        "TTS with text optimization took {:?}",
        std::time::Instant::now() - now
    );
    let header = hound::WavReader::new(wav_audio.as_ref()).unwrap();
    let spec = header.spec();
    println!("wav header: {:?}", spec);
    assert_eq!(spec.sample_rate, 16000);
    std::fs::write("./resources/test/out_opt.wav", wav_audio).unwrap();
}

#[tokio::test]
async fn test_gsv_with_opt_en() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    let tts_url = std::env::var("GSV_TTS_URL").unwrap();
    let tts_speaker = std::env::var("GSV_TTS_SPEAKER").unwrap_or_else(|_| "ad".to_string());

    let llm_voice_opt = crate::config::TTSTextOptimizationConfig {
        url: std::env::var("GSV_TTS_OPT_LLM_URL").unwrap(),
        api_key: std::env::var("GSV_TTS_OPT_LLM_API_KEY").unwrap(),
        model: std::env::var("GSV_TTS_OPT_LLM_MODEL")
            .unwrap_or_else(|_| "openai/gpt-4o-mini".to_string()),
        prompts: crate::config::TTSTextOptimizationConfig::default_prompt(),
    };

    let text = "2026 is a year full of hope. Visit https://example.com for more information. 1234M is a large number.";
    let client = reqwest::Client::new();

    let now = std::time::Instant::now();
    let wav_audio = gsv(
        &client,
        &tts_url,
        &tts_speaker,
        text,
        Some(16000),
        Some(&llm_voice_opt),
    )
    .await
    .unwrap();
    println!(
        "TTS with text optimization took {:?}",
        std::time::Instant::now() - now
    );
    let header = hound::WavReader::new(wav_audio.as_ref()).unwrap();
    let spec = header.spec();
    println!("wav header: {:?}", spec);
    assert_eq!(spec.sample_rate, 16000);
    std::fs::write("./resources/test/out_opt_en.wav", wav_audio).unwrap();
}

/// return: pcm_chunk: 16bit,32k,single-channel.
pub async fn stream_gsv(
    client: &reqwest::Client,
    tts_url: &str,
    speaker: &str,
    text: &str,
    sample_rate: Option<usize>,
    llm_voice_opt: Option<&crate::config::TTSTextOptimizationConfig>,
) -> anyhow::Result<reqwest::Response> {
    log::debug!("speaker: {speaker}, text: {text}");
    let mut text = text.to_string();

    if let Some(opt) = llm_voice_opt {
        let mut optimized_text_response = crate::ai::llm_stable(
            &opt.url,
            &opt.api_key,
            &opt.model,
            None,
            opt.prompts
                .iter()
                .chain(std::iter::once(&crate::ai::llm::Content {
                    role: crate::ai::llm::Role::User,
                    message: text.to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                })),
            Vec::new(),
        )
        .await?;
        let mut optimized_text = String::new();
        while let Ok(chunk) = optimized_text_response.next_chunk().await {
            match chunk {
                crate::ai::StableLLMResponseChunk::Functions(..) => {
                    #[cfg(debug_assertions)]
                    unreachable!("TTS text optimization should not return function calls");
                }
                crate::ai::StableLLMResponseChunk::Text(chunk) => {
                    optimized_text.push_str(&chunk);
                }
                crate::ai::StableLLMResponseChunk::Stop => {
                    break;
                }
            }
        }
        log::debug!("optimized text: {}", optimized_text);
        text = optimized_text;
    }

    let res = client
        .post(tts_url)
        .json(&serde_json::json!({"speaker": speaker, "input": text, "sample_rate": sample_rate}))
        // .body(serde_json::json!({"speaker": speaker, "input": text}).to_string())
        .send()
        .await?;
    let status = res.status();
    if status != 200 {
        let body = res.text().await?;
        return Err(anyhow::anyhow!(
            "tts failed, status:{status}, body:{}",
            body
        ));
    }
    Ok(res)
}

/// return: wav_audio: 16bit,48k,single-channel.
pub async fn groq(
    client: &reqwest::Client,
    url: &str,
    model: &str,
    token: &str,
    voice: &str,
    text: &str,
) -> anyhow::Result<Bytes> {
    let url = if url.is_empty() {
        "https://api.groq.com/openai/v1/audio/speech"
    } else {
        url
    };

    log::debug!("groq tts. voice: {voice}, text: {text}");
    let res = client
        .post(url)
        .bearer_auth(token)
        .json(&serde_json::json!({
            "model":model,
            "voice": voice,
            "input": text,
            "response_format": "wav"
        }))
        .send()
        .await?;
    let status = res.status();
    if status != 200 {
        let body = res.text().await?;
        return Err(anyhow::anyhow!(
            "tts failed, status:{status}, body:{}",
            body
        ));
    }
    let bytes = res.bytes().await?;
    log::info!("TTS response: {:?}", bytes.len());
    Ok(bytes)
}

// cargo test --package echokit_server --bin echokit_server -- ai::tts::test_groq --exact --show-output
#[tokio::test]
async fn test_groq() {
    let token = std::env::var("GROQ_API_KEY").unwrap();
    let speaker = "Aaliyah-PlayAI";
    let text = "你好，我是胡桃";
    let client = reqwest::Client::new();
    let wav_audio = groq(&client, "", "playai-tts", &token, speaker, text)
        .await
        .unwrap();
    let mut reader = wav_io::reader::Reader::from_vec(wav_audio.to_vec()).unwrap();
    let head = reader.read_header().unwrap();
    println!("wav header: {:?}", head);
    std::fs::write("./resources/test/groq_out.wav", wav_audio).unwrap();
    let samples = crate::util::get_samples_f32(&mut reader).unwrap();
    println!("samples len: {}", samples.len());
}

pub async fn openai_tts(
    client: &reqwest::Client,
    url: &str,
    model: &str,
    token: &str,
    voice: &str,
    text: &str,
) -> anyhow::Result<Bytes> {
    let url = if url.is_empty() {
        "https://api.openai.com/v1/audio/speech"
    } else {
        url
    };

    log::debug!("openai tts. voice: {voice}, text: {text}, model: {model}, url: {url}");

    let res = client
        .post(url)
        .bearer_auth(token)
        .json(&serde_json::json!({
            "model":model,
            "voice": voice,
            "input": text,
            "response_format": "wav"
        }))
        .send()
        .await?;
    let status = res.status();
    if status != 200 {
        let body = res.text().await?;
        return Err(anyhow::anyhow!(
            "tts failed, status:{status}, body:{}",
            body
        ));
    }
    let bytes = res.bytes().await?;
    log::info!("TTS response: {:?}", bytes.len());
    Ok(bytes)
}

#[derive(Debug, serde::Serialize)]
struct FishTTSRequest {
    text: String,
    chunk_length: usize,
    format: String,
    mp3_bitrate: usize,
    reference_id: String,
    normalize: bool,
    latency: String,
}

impl FishTTSRequest {
    fn new(speaker: String, text: String, format: String) -> Self {
        Self {
            text,
            chunk_length: 200,
            format,
            mp3_bitrate: 128,
            reference_id: speaker,
            normalize: true,
            latency: "normal".to_string(),
        }
    }
}

pub async fn fish_tts(url: &str, token: &str, speaker: &str, text: &str) -> anyhow::Result<Bytes> {
    let client = reqwest::Client::new();
    let url = if url.is_empty() {
        "https://api.fish.audio/v1/tts"
    } else {
        url
    };
    let res = client
        .post(url)
        .header("content-type", "application/msgpack")
        .header("model", "s1")
        .header("authorization", &format!("Bearer {}", token))
        .body(rmp_serde::to_vec_named(&FishTTSRequest::new(
            speaker.to_string(),
            text.to_string(),
            "wav".to_string(),
        ))?)
        .send()
        .await?;
    let status = res.status();
    if status != 200 {
        let body = res.text().await?;
        return Err(anyhow::anyhow!(
            "tts failed, status:{}, body:{}",
            status,
            body
        ));
    }
    let bytes = res.bytes().await?;
    Ok(bytes)
}

#[tokio::test]
async fn test_fish_tts() {
    let token = std::env::var("FISH_API_KEY").unwrap();
    let speaker = "256e1a3007a74904a91d132d1e9bf0aa";
    let text = "hello fish";

    let r = rmp_serde::to_vec_named(&FishTTSRequest::new(
        speaker.to_string(),
        text.to_string(),
        "wav".to_string(),
    ));
    println!("{:x?}", r);

    let wav_audio = fish_tts("", &token, speaker, text).await.unwrap();
    std::fs::write("./resources/test/out.wav", wav_audio).unwrap();
}
