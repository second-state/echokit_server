use std::io::{Cursor, Write};

/// WAV 音频参数结构体
#[derive(Debug, Clone)]
pub struct WavConfig {
    pub sample_rate: u32,     // 采样率 (Hz)
    pub channels: u16,        // 声道数
    pub bits_per_sample: u16, // 位深度
}

impl Default for WavConfig {
    fn default() -> Self {
        Self {
            sample_rate: 24000,  // OpenAI Realtime API 默认采样率
            channels: 1,         // 单声道
            bits_per_sample: 16, // 16-bit
        }
    }
}

pub fn pcm_to_wav(pcm_data: &[u8], config: WavConfig) -> Vec<u8> {
    let mut wav_data = Vec::new();
    let mut cursor = Cursor::new(&mut wav_data);

    let bytes_per_sample = config.bits_per_sample / 8;
    let byte_rate = config.sample_rate * config.channels as u32 * bytes_per_sample as u32;
    let block_align = config.channels * bytes_per_sample;
    let data_size = pcm_data.len() as u32;
    let file_size = 36 + data_size;

    cursor.write_all(b"RIFF").unwrap(); // ChunkID
    cursor.write_all(&file_size.to_le_bytes()).unwrap(); // ChunkSize (little-endian)
    cursor.write_all(b"WAVE").unwrap(); // Format

    // fmt 子块
    cursor.write_all(b"fmt ").unwrap(); // Subchunk1ID
    cursor.write_all(&16u32.to_le_bytes()).unwrap(); // Subchunk1Size (PCM = 16)
    cursor.write_all(&1u16.to_le_bytes()).unwrap(); // AudioFormat (PCM = 1)
    cursor.write_all(&config.channels.to_le_bytes()).unwrap(); // NumChannels
    cursor.write_all(&config.sample_rate.to_le_bytes()).unwrap(); // SampleRate
    cursor.write_all(&byte_rate.to_le_bytes()).unwrap(); // ByteRate
    cursor.write_all(&block_align.to_le_bytes()).unwrap(); // BlockAlign
    cursor
        .write_all(&config.bits_per_sample.to_le_bytes())
        .unwrap(); // BitsPerSample

    // data 子块
    cursor.write_all(b"data").unwrap(); // Subchunk2ID
    cursor.write_all(&data_size.to_le_bytes()).unwrap(); // Subchunk2Size

    // 写入 PCM 数据
    cursor.write_all(pcm_data).unwrap();

    wav_data
}

pub fn convert_samples_f32_to_i16_bytes(samples: &[f32]) -> Vec<u8> {
    let mut samples_i16 = vec![];
    for v in samples {
        let sample = (*v * std::i16::MAX as f32) as i16;
        samples_i16.extend_from_slice(&sample.to_le_bytes());
    }
    samples_i16
}
