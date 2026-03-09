pub const ITEM_ROOTFSFAT16: &str = "RFSFAT16";

#[derive(Debug, Clone)]
pub struct PartitionDownloadInfo {
    pub partition_name: String,
    pub partition_address: u64,
    pub download_filename: String,
    pub download_subtype: String,
    pub data_offset: u64,
    pub data_length: u64,
}

pub struct IncrementalChecksum {
    sum: u32,
    pending_bytes: Vec<u8>,
}

impl IncrementalChecksum {
    pub fn new() -> Self {
        IncrementalChecksum {
            sum: 0,
            pending_bytes: Vec::new(),
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        let buffer = if !self.pending_bytes.is_empty() {
            let mut combined = self.pending_bytes.clone();
            combined.extend_from_slice(data);
            self.pending_bytes.clear();
            combined
        } else {
            data.to_vec()
        };

        let aligned_length = buffer.len() & !0x03;
        let remaining = buffer.len() & 0x03;

        for i in (0..aligned_length).step_by(4) {
            let value =
                u32::from_le_bytes([buffer[i], buffer[i + 1], buffer[i + 2], buffer[i + 3]]);
            self.sum = self.sum.wrapping_add(value);
        }

        if remaining > 0 {
            self.pending_bytes = buffer[aligned_length..].to_vec();
        }
    }

    pub fn finalize(&mut self) -> u32 {
        if !self.pending_bytes.is_empty() {
            let last_value: u32 = match self.pending_bytes.len() {
                1 => self.pending_bytes[0] as u32 & 0x000000ff,
                2 => {
                    (self.pending_bytes[0] as u32 | (self.pending_bytes[1] as u32) << 8)
                        & 0x0000ffff
                }
                3 => {
                    (self.pending_bytes[0] as u32
                        | (self.pending_bytes[1] as u32) << 8
                        | (self.pending_bytes[2] as u32) << 16)
                        & 0x00ffffff
                }
                _ => 0,
            };
            self.sum = self.sum.wrapping_add(last_value);
            self.pending_bytes.clear();
        }
        self.sum
    }
}

impl Default for IncrementalChecksum {
    fn default() -> Self {
        Self::new()
    }
}
