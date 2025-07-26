// --- packages/protocol/src/lib.rs ---

// AI-MOD-START
//! 定义了 NeuroCam 项目中用于网络传输的UDP分片与重组协议。

// 包头的大小 (u32: 4 bytes + u16: 2 bytes + u16: 2 bytes = 8 bytes)
pub const HEADER_SIZE: usize = 8;
// 为了给IP和UDP头留出空间，并避免大多数网络环境下的分片，我们将单个UDP包的负载限制在一个安全的大小。
// 常见的MTU是1500字节，减去IP头(20)和UDP头(8)，剩下1472。我们选择一个更保守的值。
pub const MAX_PAYLOAD_SIZE: usize = 1400;

/// UDP 数据包的头部结构。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketHeader {
    /// 当前帧的唯一标识符。
    pub frame_id: u32,
    /// 当前分片在此帧中的序号 (从 0 开始)。
    pub packet_id: u16,
    /// 此帧总共被分成了多少个包。
    pub total_packets: u16,
}

impl PacketHeader {
    /// 将头部序列化为字节数组（网络字节序 - Big Endian）。
    pub fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut bytes = [0u8; HEADER_SIZE];
        bytes[0..4].copy_from_slice(&self.frame_id.to_be_bytes());
        bytes[4..6].copy_from_slice(&self.packet_id.to_be_bytes());
        bytes[6..8].copy_from_slice(&self.total_packets.to_be_bytes());
        bytes
    }

    /// 从字节数组中反序列化出头部。
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < HEADER_SIZE {
            return None;
        }
        let frame_id = u32::from_be_bytes(bytes[0..4].try_into().ok()?);
        let packet_id = u16::from_be_bytes(bytes[4..6].try_into().ok()?);
        let total_packets = u16::from_be_bytes(bytes[6..8].try_into().ok()?);
        Some(PacketHeader {
            frame_id,
            packet_id,
            total_packets,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let header = PacketHeader {
            frame_id: 12345,
            packet_id: 1,
            total_packets: 10,
        };

        let bytes = header.to_bytes();
        let reconstructed_header = PacketHeader::from_bytes(&bytes).unwrap();

        assert_eq!(header, reconstructed_header);
    }

    #[test]
    fn test_from_bytes_with_insufficient_length() {
        let bytes = [0u8; 7];
        assert!(PacketHeader::from_bytes(&bytes).is_none());
    }
}
// AI-MOD-END
