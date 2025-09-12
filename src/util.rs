use std::io::{Cursor, Write};

use wav_io::{header::SampleFormat, reader::DecodeError};

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
    cursor.write_all(&config.bits_per_sample.to_le_bytes()).unwrap(); // BitsPerSample

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
        let sample = (*v * i16::MAX as f32) as i16;
        samples_i16.extend_from_slice(&sample.to_le_bytes());
    }
    samples_i16
}

pub fn get_samples_f32(reader: &mut wav_io::reader::Reader) -> Result<Vec<f32>, DecodeError> {
    let mut result: Vec<f32> = Vec::new();
    loop {
        // read chunks
        let chunk_tag = reader.read_str4();
        if chunk_tag.is_empty() {
            break;
        }
        let size = reader.read_u32().unwrap_or(0) as u64;
        // todo: check tag
        // println!("[info] tag={:?}::{}", chunk_tag, size);
        if size == 0 {
            continue;
        }
        // data?
        if chunk_tag != "data" {
            reader.cur.set_position(reader.cur.position() + size);
            continue;
        }
        // read wav data
        let h = &reader.header.clone().unwrap();

        let bytes_to_read = if size == 0xFFFFFFFF {
            let current_pos = reader.cur.position();
            let file_len = reader.cur.get_ref().len() as u64;
            file_len.saturating_sub(current_pos)
        } else {
            size
        };

        let bytes_per_sample = (h.bits_per_sample / 8) as u64;
        let total_samples = bytes_to_read / bytes_per_sample;
        if result.is_empty() {
            result = Vec::with_capacity(total_samples as usize);
        }

        match h.sample_format {
            // float
            SampleFormat::Float => {
                match h.bits_per_sample {
                    32 => {
                        for _ in 0..total_samples {
                            let lv = reader.read_f32().unwrap_or(0.0);
                            result.push(lv);
                        }
                    },
                    64 => {
                        for _ in 0..total_samples {
                            let lv = reader.read_f64().unwrap_or(0.0);
                            result.push(lv as f32); // down to f32
                        }
                    },
                    _ => {
                        return Err(DecodeError::UnsupportedWav {
                            attribute: "bits per float sample",
                            expected: &[32, 64],
                            found: h.bits_per_sample as u32,
                        })
                    },
                }
            },
            // int
            SampleFormat::Int => {
                match h.bits_per_sample {
                    8 => {
                        for _ in 0..total_samples {
                            // 0..255
                            let lv = reader.read_u8().unwrap_or(0);
                            let fv = lv.wrapping_sub(128) as i8 as f32 / (i8::MAX as f32);
                            result.push(fv);
                        }
                    },
                    16 => {
                        for _ in 0..total_samples {
                            let lv = reader.read_i16().unwrap_or(0);
                            let fv = lv as f32 / (i16::MAX as f32);
                            result.push(fv);
                        }
                    },
                    24 => {
                        for _ in 0..total_samples {
                            let lv = reader.read_i24().unwrap_or(0);
                            let fv = lv as f32 / ((1 << 23) - 1) as f32;
                            result.push(fv);
                        }
                    },
                    32 => {
                        for _ in 0..total_samples {
                            let lv = reader.read_i32().unwrap_or(0);
                            let fv = lv as f32 / (i32::MAX as f32);
                            result.push(fv);
                        }
                    },
                    _ => {
                        return Err(DecodeError::UnsupportedWav {
                            attribute: "bits per integer sample",
                            expected: &[8, 16, 24, 32],
                            found: h.bits_per_sample as u32,
                        })
                    },
                }
            },
            _ => return Err(DecodeError::UnsupportedEncoding),
        }
    }
    Ok(result)
}

pub fn get_samples_i16(reader: &mut wav_io::reader::Reader) -> Result<Vec<i16>, DecodeError> {
    let mut result: Vec<i16> = Vec::new();
    loop {
        // read chunks
        let chunk_tag = reader.read_str4();
        if chunk_tag.is_empty() {
            break;
        }
        let size = reader.read_u32().unwrap_or(0) as u64;
        // todo: check tag
        // println!("[info] tag={:?}::{}", chunk_tag, size);
        if size == 0 {
            continue;
        }
        // data?
        if chunk_tag != "data" {
            reader.cur.set_position(reader.cur.position() + size);
            continue;
        }
        // read wav data
        let h = &reader.header.clone().unwrap();

        let bytes_to_read = if size == 0xFFFFFFFF {
            let current_pos = reader.cur.position();
            let file_len = reader.cur.get_ref().len() as u64;
            file_len.saturating_sub(current_pos)
        } else {
            size
        };

        let bytes_per_sample = (h.bits_per_sample / 8) as u64;
        let total_samples = bytes_to_read / bytes_per_sample;
        if result.is_empty() {
            result = Vec::with_capacity(total_samples as usize);
        }

        match h.sample_format {
            // float
            SampleFormat::Float => match h.bits_per_sample {
                32 => {
                    for _ in 0..total_samples {
                        let lv = reader.read_f32().unwrap_or(0.0);
                        let sample = (lv.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                        result.push(sample);
                    }
                },
                64 => {
                    for _ in 0..total_samples {
                        let lv = reader.read_f64().unwrap_or(0.0);
                        let sample = ((lv as f32).clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                        result.push(sample);
                    }
                },
                _ => {
                    return Err(DecodeError::UnsupportedWav {
                        attribute: "bits per float sample",
                        expected: &[32, 64],
                        found: h.bits_per_sample as u32,
                    })
                },
            },
            // int
            SampleFormat::Int => match h.bits_per_sample {
                8 => {
                    for _ in 0..total_samples {
                        let lv = reader.read_u8().unwrap_or(0);
                        let normalized = (lv as f32) / (i8::MAX as f32);
                        let sample = (normalized * i16::MAX as f32) as i16;
                        result.push(sample);
                    }
                },
                16 => {
                    for _ in 0..total_samples {
                        let lv = reader.read_i16().unwrap_or(0);
                        result.push(lv);
                    }
                },
                24 => {
                    for _ in 0..total_samples {
                        let lv = reader.read_i24().unwrap_or(0);
                        let normalized = lv as f32 / 0xFFFFFF as f32;
                        let sample = (normalized * i16::MAX as f32) as i16;
                        result.push(sample);
                    }
                },
                32 => {
                    for _ in 0..total_samples {
                        let lv = reader.read_i32().unwrap_or(0);
                        let normalized = lv as f32 / i32::MAX as f32;
                        let sample = (normalized * i16::MAX as f32) as i16;
                        result.push(sample);
                    }
                },
                _ => {
                    return Err(DecodeError::UnsupportedWav {
                        attribute: "bits per integer sample",
                        expected: &[8, 16, 24, 32],
                        found: h.bits_per_sample as u32,
                    })
                },
            },
            _ => return Err(DecodeError::UnsupportedEncoding),
        }
    }
    Ok(result)
}
