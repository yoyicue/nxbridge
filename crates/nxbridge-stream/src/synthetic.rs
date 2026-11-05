//! Deterministic test source using the shared NXBridge media descriptors.
use nxbridge_media::{
    AudioBuffer, AudioChunk, Epoch, Plane, VideoBuffer, VideoFrame, ABI_V1, AUDIO_CHANNELS,
    AUDIO_F32LE_INTERLEAVED, AUDIO_FRAMES_MAX, AUDIO_RATE, COLOR_BT709, MATRIX_RGB, RANGE_FULL,
    READY_CPU_COMPLETE, STORAGE_CPU_SHARED, TRANSFER_BT709, VIDEO_BGRA8,
};
use std::mem::size_of;

pub const FRAME_RATE: u64 = 30;
pub const AUDIO_CHUNK_FRAMES: u64 = AUDIO_FRAMES_MAX as u64;
const DIGITS: [[u8; 5]; 10] = [
    [0x1f, 0x11, 0x11, 0x11, 0x1f],
    [0x00, 0x12, 0x1f, 0x10, 0x00],
    [0x1d, 0x15, 0x15, 0x15, 0x17],
    [0x11, 0x15, 0x15, 0x15, 0x1f],
    [0x07, 0x04, 0x04, 0x04, 0x1f],
    [0x17, 0x15, 0x15, 0x15, 0x1d],
    [0x1f, 0x15, 0x15, 0x15, 0x1d],
    [0x01, 0x01, 0x01, 0x01, 0x1f],
    [0x1f, 0x15, 0x15, 0x15, 0x1f],
    [0x17, 0x15, 0x15, 0x15, 0x1f],
];

#[derive(Debug)]
pub enum SourceError {
    InvalidEpoch,
    InvalidSize,
    InvalidBufferId,
    Overflow,
}

pub struct VideoSample {
    pub descriptor: VideoFrame,
    pub buffer: VideoBuffer,
    pub pixels: Vec<u8>,
}
pub struct AudioSample {
    pub descriptor: AudioChunk,
    pub buffer: AudioBuffer,
    pub pcm_le: Vec<u8>,
}

pub struct SyntheticSource {
    epoch: Epoch,
    origin_ns: u64,
    width: u32,
    height: u32,
}
impl SyntheticSource {
    pub fn new(epoch: Epoch, origin_ns: u64, width: u32, height: u32) -> Result<Self, SourceError> {
        epoch.validate().map_err(|_| SourceError::InvalidEpoch)?;
        if !(176..=1920).contains(&width) || !(90..=1080).contains(&height) {
            return Err(SourceError::InvalidSize);
        }
        Ok(Self {
            epoch,
            origin_ns,
            width,
            height,
        })
    }
    fn timestamp(&self, n: u64, rate: u64) -> Result<u64, SourceError> {
        let ns = u128::from(n) * 1_000_000_000 / u128::from(rate) + u128::from(self.origin_ns);
        u64::try_from(ns).map_err(|_| SourceError::Overflow)
    }
    pub fn video(&self, index: u64, buffer_id: u64) -> Result<VideoSample, SourceError> {
        if buffer_id == 0 {
            return Err(SourceError::InvalidBufferId);
        }
        let next = index.checked_add(1).ok_or(SourceError::Overflow)?;
        let pts = self.timestamp(index, FRAME_RATE)?;
        let end = self.timestamp(next, FRAME_RATE)?;
        let stride = self.width as usize * 4;
        let mut pixels = vec![0; stride * self.height as usize];
        let moving_x = (index % u64::from(self.width)) as u32;
        let flash = index % FRAME_RATE < 3;
        for y in 0..self.height {
            for x in 0..self.width {
                let offset = y as usize * stride + x as usize * 4;
                let on_box = x >= moving_x
                    && x < moving_x.saturating_add(48).min(self.width)
                    && y >= self.height / 2
                    && y < self.height / 2 + 48;
                let color = if on_box {
                    [32, 210, 255, 255]
                } else if flash && x < 24 && y < 24 {
                    [255, 255, 255, 255]
                } else {
                    [
                        (x * 90 / self.width + 20) as u8,
                        (y * 90 / self.height + 20) as u8,
                        32,
                        255,
                    ]
                };
                pixels[offset..offset + 4].copy_from_slice(&color);
            }
        }
        // Render a six-digit frame counter with a compact pixel font.
        let digits = format!("{:06}", index % 1_000_000);
        for (i, b) in digits.bytes().enumerate() {
            let glyph = DIGITS[(b - b'0') as usize];
            for (column, bits) in glyph.into_iter().enumerate() {
                for row in 0..5 {
                    if bits & (1 << row) != 0 {
                        for sy in 0..4 {
                            for sx in 0..4 {
                                let x = 32 + i * 24 + column * 4 + sx;
                                let y = 32 + row * 4 + sy;
                                let offset = y * stride + x * 4;
                                pixels[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
                            }
                        }
                    }
                }
            }
        }
        let length = pixels.len() as u64;
        let descriptor = VideoFrame {
            struct_size: size_of::<VideoFrame>() as u32,
            abi_version: ABI_V1,
            epoch: self.epoch,
            sequence: next,
            pts_ns: pts,
            duration_ns: end - pts,
            buffer_id,
            pixel_format: VIDEO_BGRA8,
            width: self.width,
            height: self.height,
            color_primaries: COLOR_BT709,
            transfer: TRANSFER_BT709,
            matrix: MATRIX_RGB,
            color_range: RANGE_FULL,
            plane_count: 1,
            ready_kind: READY_CPU_COMPLETE,
            reserved: [0; 3],
        };
        let buffer = VideoBuffer {
            struct_size: size_of::<VideoBuffer>() as u32,
            abi_version: ABI_V1,
            epoch: self.epoch,
            buffer_id,
            allocation_bytes: length,
            storage_kind: STORAGE_CPU_SHARED,
            pixel_format: VIDEO_BGRA8,
            width: self.width,
            height: self.height,
            plane_count: 1,
            reserved: 0,
            planes: [
                Plane {
                    offset_bytes: 0,
                    length_bytes: length,
                    stride_bytes: stride as u32,
                    reserved: 0,
                },
                Plane::default(),
            ],
        };
        Ok(VideoSample {
            descriptor,
            buffer,
            pixels,
        })
    }
    pub fn audio(&self, index: u64, buffer_id: u64) -> Result<AudioSample, SourceError> {
        if buffer_id == 0 {
            return Err(SourceError::InvalidBufferId);
        }
        let sequence = index.checked_add(1).ok_or(SourceError::Overflow)?;
        let first_sample = index
            .checked_mul(AUDIO_CHUNK_FRAMES)
            .ok_or(SourceError::Overflow)?;
        let pts = self.timestamp(first_sample, u64::from(AUDIO_RATE))?;
        let mut pcm_le = Vec::with_capacity(AUDIO_CHUNK_FRAMES as usize * 8);
        for offset in 0..AUDIO_CHUNK_FRAMES {
            let sample = first_sample
                .checked_add(offset)
                .ok_or(SourceError::Overflow)?;
            // 100 ms of 880 Hz once per second, aligned with the video flash.
            let amplitude = if sample % u64::from(AUDIO_RATE) < u64::from(AUDIO_RATE) / 10 {
                (2.0 * std::f64::consts::PI * 880.0 * (sample % u64::from(AUDIO_RATE)) as f64
                    / f64::from(AUDIO_RATE))
                .sin() as f32
                    * 0.2
            } else {
                0.0
            };
            for _ in 0..AUDIO_CHANNELS {
                pcm_le.extend_from_slice(&amplitude.to_le_bytes());
            }
        }
        let descriptor = AudioChunk {
            struct_size: size_of::<AudioChunk>() as u32,
            abi_version: ABI_V1,
            epoch: self.epoch,
            sequence,
            pts_ns: pts,
            first_sample,
            buffer_id,
            sample_format: AUDIO_F32LE_INTERLEAVED,
            sample_rate: AUDIO_RATE,
            channels: AUDIO_CHANNELS,
            frames: AUDIO_FRAMES_MAX,
            flags: 0,
            reserved: 0,
        };
        let buffer = AudioBuffer {
            struct_size: size_of::<AudioBuffer>() as u32,
            abi_version: ABI_V1,
            epoch: self.epoch,
            buffer_id,
            allocation_bytes: pcm_le.len() as u64,
            storage_kind: STORAGE_CPU_SHARED,
            reserved: 0,
        };
        Ok(AudioSample {
            descriptor,
            buffer,
            pcm_le,
        })
    }
}
