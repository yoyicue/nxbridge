//! Bounded H.264 Annex-B access-unit splitting for an encoder emitting AUDs.
//!
//! This is a byte-stream adapter, not a complete H.264 parser. The encoder
//! must insert an access-unit delimiter before every frame and repeat SPS/PPS
//! for independently decodable keyframes.

/// Maximum encoded access-unit size retained by the adapter.
pub const MAX_ACCESS_UNIT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum AnnexBError {
    /// Bytes preceding the first access-unit delimiter are not accepted.
    MissingDelimiter,
    /// The source did not produce a delimiter within the buffer limit.
    AccessUnitTooLarge,
    /// End of input occurred in the middle of an empty delimiter.
    IncompleteAccessUnit,
}

#[derive(Debug, PartialEq, Eq)]
pub struct AccessUnit {
    pub data: Vec<u8>,
    pub is_key_frame: bool,
}

/// Accepts arbitrary pipe-read boundaries and emits whole access units.
#[derive(Default)]
pub struct AnnexBSplitter {
    buffer: Vec<u8>,
}

// Check both legal Annex-B prefixes, preferring the four-byte form. Never
// consume a prefix until the NAL header is present: the next read may supply it.
fn nal_start(data: &[u8], from: usize) -> Option<(usize, usize, u8)> {
    let mut i = from;
    while i + 3 < data.len() {
        let prefix = if data[i..].starts_with(&[0, 0, 0, 1]) {
            4
        } else if data[i..].starts_with(&[0, 0, 1]) {
            3
        } else {
            i += 1;
            continue;
        };
        if i + prefix >= data.len() {
            return None;
        }
        return Some((i, prefix, data[i + prefix] & 0x1f));
    }
    None
}

fn access_unit(data: Vec<u8>) -> AccessUnit {
    let mut cursor = 0;
    let mut is_key_frame = false;
    while let Some((start, prefix, kind)) = nal_start(&data, cursor) {
        is_key_frame |= kind == 5;
        cursor = start + prefix + 1;
    }
    AccessUnit { data, is_key_frame }
}

impl AnnexBSplitter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a bounded chunk. A single pipe read is limited to 64 KiB, so a
    /// caller cannot force an unbounded allocation before a delimiter appears.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<AccessUnit>, AnnexBError> {
        if chunk.len() > 64 * 1024
            || self.buffer.len().saturating_add(chunk.len()) > MAX_ACCESS_UNIT_BYTES + 64 * 1024
        {
            return Err(AnnexBError::AccessUnitTooLarge);
        }
        self.buffer.extend_from_slice(chunk);
        let mut out = Vec::new();
        loop {
            let Some((start, prefix, kind)) = nal_start(&self.buffer, 0) else {
                if self.buffer.len() > MAX_ACCESS_UNIT_BYTES {
                    return Err(AnnexBError::AccessUnitTooLarge);
                }
                break;
            };
            if start != 0 || kind != 9 {
                return Err(AnnexBError::MissingDelimiter);
            }
            let mut cursor = prefix + 1;
            let mut next = None;
            while let Some((i, p, k)) = nal_start(&self.buffer, cursor) {
                if k == 9 {
                    next = Some(i);
                    break;
                }
                cursor = i + p + 1;
            }
            match next {
                Some(i) => {
                    if i > MAX_ACCESS_UNIT_BYTES {
                        return Err(AnnexBError::AccessUnitTooLarge);
                    }
                    let tail = self.buffer.split_off(i);
                    let complete = std::mem::replace(&mut self.buffer, tail);
                    out.push(access_unit(complete));
                }
                None => {
                    if self.buffer.len() > MAX_ACCESS_UNIT_BYTES {
                        return Err(AnnexBError::AccessUnitTooLarge);
                    }
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Flush the final frame only on an orderly encoder EOF.
    pub fn finish(&mut self) -> Result<Option<AccessUnit>, AnnexBError> {
        if self.buffer.is_empty() {
            return Ok(None);
        }
        let Some((0, prefix, 9)) = nal_start(&self.buffer, 0) else {
            return Err(AnnexBError::MissingDelimiter);
        };
        if nal_start(&self.buffer, prefix + 1).is_none() {
            return Err(AnnexBError::IncompleteAccessUnit);
        }
        Ok(Some(access_unit(std::mem::take(&mut self.buffer))))
    }
}
