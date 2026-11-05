//! Versioned source-to-encoder descriptors and bounded video lease tracking.
//! Handles are registered out-of-band; descriptors are native ABI, not IPC bytes.
use std::collections::{HashMap, VecDeque};

pub const ABI_V1: u32 = 1;
pub const VIDEO_BGRA8: u32 = 1;
pub const VIDEO_NV12: u32 = 2;
pub const COLOR_BT709: u32 = 1;
pub const TRANSFER_BT709: u32 = 1;
pub const MATRIX_RGB: u32 = 1;
pub const MATRIX_BT709: u32 = 2;
pub const RANGE_FULL: u32 = 1;
pub const RANGE_VIDEO: u32 = 2;
pub const STORAGE_CPU_SHARED: u32 = 1;
pub const STORAGE_IOSURFACE: u32 = 2;
pub const READY_CPU_COMPLETE: u32 = 1;
pub const READY_GPU_COMPLETE: u32 = 2;
pub const AUDIO_F32LE_INTERLEAVED: u32 = 1;
pub const VIDEO_PENDING_MAX: usize = 3;
pub const VIDEO_INFLIGHT_MAX: usize = 2;
pub const VIDEO_BUFFERS_MAX: usize = 6;
pub const AUDIO_CHUNKS_MAX: usize = 20;
pub const AUDIO_INFLIGHT_MAX: usize = 1;
pub const AUDIO_BUFFERS_MAX: usize = 22;
pub const AUDIO_DISCONTINUITY: u32 = 1;
pub const AUDIO_RATE: u32 = 48_000;
pub const AUDIO_CHANNELS: u32 = 2;
pub const AUDIO_FRAMES_MAX: u32 = 480;
pub const RELEASE_ENCODED: u32 = 1;
pub const RELEASE_DROPPED: u32 = 2;
pub const RELEASE_FAILED: u32 = 3;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Epoch {
    pub session_id: [u8; 16],
    pub generation: u64,
}
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Plane {
    pub offset_bytes: u64,
    pub length_bytes: u64,
    pub stride_bytes: u32,
    pub reserved: u32,
}
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoBuffer {
    pub struct_size: u32,
    pub abi_version: u32,
    pub epoch: Epoch,
    pub buffer_id: u64,
    pub allocation_bytes: u64,
    pub storage_kind: u32,
    pub pixel_format: u32,
    pub width: u32,
    pub height: u32,
    pub plane_count: u32,
    pub reserved: u32,
    pub planes: [Plane; 2],
}
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoFrame {
    pub struct_size: u32,
    pub abi_version: u32,
    pub epoch: Epoch,
    pub sequence: u64,
    pub pts_ns: u64,
    pub duration_ns: u64,
    pub buffer_id: u64,
    pub pixel_format: u32,
    pub width: u32,
    pub height: u32,
    pub color_primaries: u32,
    pub transfer: u32,
    pub matrix: u32,
    pub color_range: u32,
    pub plane_count: u32,
    pub ready_kind: u32,
    pub reserved: [u32; 3],
}
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioBuffer {
    pub struct_size: u32,
    pub abi_version: u32,
    pub epoch: Epoch,
    pub buffer_id: u64,
    pub allocation_bytes: u64,
    pub storage_kind: u32,
    pub reserved: u32,
}
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioChunk {
    pub struct_size: u32,
    pub abi_version: u32,
    pub epoch: Epoch,
    pub sequence: u64,
    pub pts_ns: u64,
    pub first_sample: u64,
    pub buffer_id: u64,
    pub sample_format: u32,
    pub sample_rate: u32,
    pub channels: u32,
    pub frames: u32,
    pub flags: u32,
    pub reserved: u32,
}
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoRelease {
    pub struct_size: u32,
    pub abi_version: u32,
    pub epoch: Epoch,
    pub sequence: u64,
    pub buffer_id: u64,
    pub disposition: u32,
    pub reserved: u32,
}
pub type AudioRelease = VideoRelease;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Invalid,
    WrongEpoch,
    OutOfOrder,
    BufferBusy,
    Capacity,
    Closing,
    UnknownLease,
}

impl Epoch {
    pub fn validate(self) -> Result<(), Error> {
        if self.generation == 0 || self.session_id == [0; 16] {
            Err(Error::Invalid)
        } else {
            Ok(())
        }
    }
}
impl VideoBuffer {
    pub fn validate(&self) -> Result<(), Error> {
        self.epoch.validate()?;
        let shape = match self.pixel_format {
            VIDEO_BGRA8 => self.plane_count == 1,
            VIDEO_NV12 => self.plane_count == 2 && self.width % 2 == 0 && self.height % 2 == 0,
            _ => false,
        };
        if self.struct_size != std::mem::size_of::<Self>() as u32
            || self.abi_version != ABI_V1
            || self.buffer_id == 0
            || self.allocation_bytes == 0
            || !matches!(self.storage_kind, STORAGE_CPU_SHARED | STORAGE_IOSURFACE)
            || self.width == 0
            || self.height == 0
            || self.width > 8192
            || self.height > 8192
            || !shape
            || self.reserved != 0
        {
            return Err(Error::Invalid);
        }
        for (index, p) in self.planes.iter().enumerate() {
            if index >= self.plane_count as usize {
                if *p != Plane::default() {
                    return Err(Error::Invalid);
                }
                continue;
            }
            let row = if self.pixel_format == VIDEO_BGRA8 {
                self.width * 4
            } else {
                self.width
            };
            let rows = if self.pixel_format == VIDEO_NV12 && index == 1 {
                self.height / 2
            } else {
                self.height
            };
            let required = u64::from(p.stride_bytes) * u64::from(rows);
            if p.reserved != 0
                || p.stride_bytes < row
                || p.length_bytes < required
                || p.offset_bytes
                    .checked_add(p.length_bytes)
                    .is_none_or(|end| end > self.allocation_bytes)
            {
                return Err(Error::Invalid);
            }
        }
        if self.plane_count == 2 {
            let first = self.planes[0];
            let second = self.planes[1];
            if first.offset_bytes < second.offset_bytes + second.length_bytes
                && second.offset_bytes < first.offset_bytes + first.length_bytes
            {
                return Err(Error::Invalid);
            }
        }
        Ok(())
    }
    pub fn validate_for(&self, frame: &VideoFrame) -> Result<(), Error> {
        self.validate()?;
        frame.validate()?;
        if self.epoch != frame.epoch
            || self.buffer_id != frame.buffer_id
            || self.pixel_format != frame.pixel_format
            || self.width != frame.width
            || self.height != frame.height
            || self.plane_count != frame.plane_count
            || (frame.ready_kind == READY_GPU_COMPLETE && self.storage_kind != STORAGE_IOSURFACE)
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
impl VideoFrame {
    pub fn validate(&self) -> Result<(), Error> {
        self.epoch.validate()?;
        let format_ok = match self.pixel_format {
            VIDEO_BGRA8 => {
                self.plane_count == 1 && self.matrix == MATRIX_RGB && self.color_range == RANGE_FULL
            }
            VIDEO_NV12 => {
                self.plane_count == 2
                    && self.width % 2 == 0
                    && self.height % 2 == 0
                    && self.matrix == MATRIX_BT709
                    && matches!(self.color_range, RANGE_FULL | RANGE_VIDEO)
            }
            _ => false,
        };
        if self.struct_size != std::mem::size_of::<Self>() as u32
            || self.abi_version != ABI_V1
            || self.sequence == 0
            || self.buffer_id == 0
            || self.duration_ns == 0
            || self.width == 0
            || self.height == 0
            || self.width > 8192
            || self.height > 8192
            || !format_ok
            || self.color_primaries != COLOR_BT709
            || self.transfer != TRANSFER_BT709
            || !matches!(self.ready_kind, READY_CPU_COMPLETE | READY_GPU_COMPLETE)
            || self.reserved != [0; 3]
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
impl AudioChunk {
    pub fn validate(&self) -> Result<(), Error> {
        self.epoch.validate()?;
        if self.struct_size != std::mem::size_of::<Self>() as u32
            || self.abi_version != ABI_V1
            || self.sequence == 0
            || self.buffer_id == 0
            || self.sample_format != AUDIO_F32LE_INTERLEAVED
            || self.sample_rate != AUDIO_RATE
            || self.channels != AUDIO_CHANNELS
            || self.frames == 0
            || self.frames > AUDIO_FRAMES_MAX
            || self.flags & !AUDIO_DISCONTINUITY != 0
            || self.reserved != 0
            || self.first_sample.checked_add(self.frames as u64).is_none()
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
impl AudioBuffer {
    pub fn validate_for(&self, chunk: &AudioChunk) -> Result<(), Error> {
        chunk.validate()?;
        if self.struct_size != std::mem::size_of::<Self>() as u32
            || self.abi_version != ABI_V1
            || self.epoch != chunk.epoch
            || self.buffer_id != chunk.buffer_id
            || self.storage_kind != STORAGE_CPU_SHARED
            || self.reserved != 0
            || self.allocation_bytes < u64::from(chunk.frames) * 2 * 4
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
fn release(f: VideoFrame, disposition: u32) -> VideoRelease {
    VideoRelease {
        struct_size: std::mem::size_of::<VideoRelease>() as u32,
        abi_version: ABI_V1,
        epoch: f.epoch,
        sequence: f.sequence,
        buffer_id: f.buffer_id,
        disposition,
        reserved: 0,
    }
}
fn audio_release(c: AudioChunk, disposition: u32) -> AudioRelease {
    AudioRelease {
        struct_size: std::mem::size_of::<AudioRelease>() as u32,
        abi_version: ABI_V1,
        epoch: c.epoch,
        sequence: c.sequence,
        buffer_id: c.buffer_id,
        disposition,
        reserved: 0,
    }
}
/// Pending frames may be dropped; frames handed to an encoder require explicit completion.
pub struct VideoLeases {
    epoch: Epoch,
    pending: VecDeque<VideoFrame>,
    inflight: HashMap<u64, VideoFrame>,
    last: Option<(u64, u64)>,
    closing: bool,
}
impl VideoLeases {
    pub fn new(epoch: Epoch) -> Result<Self, Error> {
        epoch.validate()?;
        Ok(Self {
            epoch,
            pending: VecDeque::new(),
            inflight: HashMap::new(),
            last: None,
            closing: false,
        })
    }
    /// Caller must deliver any returned release to the producer before it reuses the buffer.
    pub fn publish(&mut self, f: VideoFrame) -> Result<Option<VideoRelease>, Error> {
        if self.closing {
            return Err(Error::Closing);
        }
        f.validate()?;
        if f.epoch != self.epoch {
            return Err(Error::WrongEpoch);
        }
        if self
            .last
            .is_some_and(|(seq, pts)| f.sequence <= seq || f.pts_ns <= pts)
        {
            return Err(Error::OutOfOrder);
        }
        if self.inflight.contains_key(&f.buffer_id)
            || self.pending.iter().any(|x| x.buffer_id == f.buffer_id)
        {
            return Err(Error::BufferBusy);
        }
        if self.pending.len() + self.inflight.len() >= VIDEO_BUFFERS_MAX {
            return Err(Error::Capacity);
        }
        let dropped = if self.pending.len() == VIDEO_PENDING_MAX {
            self.pending
                .pop_front()
                .map(|x| release(x, RELEASE_DROPPED))
        } else {
            None
        };
        self.last = Some((f.sequence, f.pts_ns));
        self.pending.push_back(f);
        Ok(dropped)
    }
    pub fn begin_encode(&mut self) -> Option<VideoFrame> {
        if self.closing || self.inflight.len() >= VIDEO_INFLIGHT_MAX {
            return None;
        }
        let f = self.pending.pop_front()?;
        self.inflight.insert(f.buffer_id, f);
        Some(f)
    }
    /// Only invoke after the encoder and any GPU work are finished with the buffer.
    pub fn complete(
        &mut self,
        epoch: Epoch,
        buffer_id: u64,
        sequence: u64,
        disposition: u32,
    ) -> Result<VideoRelease, Error> {
        if epoch != self.epoch {
            return Err(Error::WrongEpoch);
        }
        if !matches!(
            disposition,
            RELEASE_ENCODED | RELEASE_DROPPED | RELEASE_FAILED
        ) {
            return Err(Error::Invalid);
        }
        let f = *self.inflight.get(&buffer_id).ok_or(Error::UnknownLease)?;
        if f.sequence != sequence {
            return Err(Error::UnknownLease);
        }
        self.inflight.remove(&buffer_id);
        Ok(release(f, disposition))
    }
    /// In-flight leases remain owned by the encoder and cannot be reclaimed here.
    pub fn close(&mut self) -> Vec<VideoRelease> {
        self.closing = true;
        self.pending
            .drain(..)
            .map(|f| release(f, RELEASE_DROPPED))
            .collect()
    }
    pub fn drained(&self) -> bool {
        self.closing && self.pending.is_empty() && self.inflight.is_empty()
    }
}

/// Audio is consumed in order; overflow rejects the new chunk without taking ownership.
pub struct AudioLeases {
    epoch: Epoch,
    pending: VecDeque<AudioChunk>,
    inflight: Option<AudioChunk>,
    // sequence, timestamp, sample end, timeline anchor sample and timestamp
    last: Option<(u64, u64, u64, u64, u64)>,
    closing: bool,
}
impl AudioLeases {
    pub fn new(epoch: Epoch) -> Result<Self, Error> {
        epoch.validate()?;
        Ok(Self {
            epoch,
            pending: VecDeque::new(),
            inflight: None,
            last: None,
            closing: false,
        })
    }
    pub fn publish(&mut self, c: AudioChunk) -> Result<(), Error> {
        if self.closing {
            return Err(Error::Closing);
        }
        c.validate()?;
        if c.epoch != self.epoch {
            return Err(Error::WrongEpoch);
        }
        if let Some((seq, pts, end, anchor_sample, anchor_pts)) = self.last {
            if c.sequence <= seq
                || c.pts_ns <= pts
                || c.first_sample < end
                || ((c.sequence - seq != 1 || c.first_sample != end)
                    && c.flags & AUDIO_DISCONTINUITY == 0)
            {
                return Err(Error::OutOfOrder);
            }
            if c.flags & AUDIO_DISCONTINUITY == 0 {
                let delta = u128::from(c.first_sample - anchor_sample) * 1_000_000_000
                    / u128::from(AUDIO_RATE);
                if u128::from(anchor_pts) + delta != u128::from(c.pts_ns) {
                    return Err(Error::OutOfOrder);
                }
            }
        }
        if self.inflight.is_some_and(|x| x.buffer_id == c.buffer_id)
            || self.pending.iter().any(|x| x.buffer_id == c.buffer_id)
        {
            return Err(Error::BufferBusy);
        }
        if self.pending.len() >= AUDIO_CHUNKS_MAX {
            return Err(Error::Capacity);
        }
        let (anchor_sample, anchor_pts) = match self.last {
            Some((_, _, _, sample, pts)) if c.flags & AUDIO_DISCONTINUITY == 0 => (sample, pts),
            _ => (c.first_sample, c.pts_ns),
        };
        self.last = Some((
            c.sequence,
            c.pts_ns,
            c.first_sample + c.frames as u64,
            anchor_sample,
            anchor_pts,
        ));
        self.pending.push_back(c);
        Ok(())
    }
    pub fn begin_consume(&mut self) -> Option<AudioChunk> {
        if self.closing || self.inflight.is_some() {
            return None;
        }
        let c = self.pending.pop_front()?;
        self.inflight = Some(c);
        Some(c)
    }
    /// Returns ownership only after copying or encoding has finished with the PCM bytes.
    pub fn complete(
        &mut self,
        epoch: Epoch,
        buffer_id: u64,
        sequence: u64,
        disposition: u32,
    ) -> Result<AudioRelease, Error> {
        if epoch != self.epoch {
            return Err(Error::WrongEpoch);
        }
        if !matches!(
            disposition,
            RELEASE_ENCODED | RELEASE_DROPPED | RELEASE_FAILED
        ) {
            return Err(Error::Invalid);
        }
        let c = self.inflight.ok_or(Error::UnknownLease)?;
        if c.buffer_id != buffer_id || c.sequence != sequence {
            return Err(Error::UnknownLease);
        }
        self.inflight = None;
        Ok(audio_release(c, disposition))
    }
    pub fn close(&mut self) -> Vec<AudioRelease> {
        self.closing = true;
        self.pending
            .drain(..)
            .map(|c| audio_release(c, RELEASE_DROPPED))
            .collect()
    }
    pub fn drained(&self) -> bool {
        self.closing && self.pending.is_empty() && self.inflight.is_none()
    }
}
