// --- packages/protocol/src/lib.rs ---

//! 定义了 NeuroCam 项目中用于网络传输的UDP分片与重组协议。
use std::mem::size_of;

// --- 通用定义 ---

/// 定义网络包的类型
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketType {
    Data = 0,
    Ack = 1,
    IFrameRequest = 2, // 新增：用于请求关键帧
}

impl TryFrom<u8> for PacketType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(PacketType::Data),
            1 => Ok(PacketType::Ack),
            2 => Ok(PacketType::IFrameRequest),
            _ => Err(()),
        }
    }
}

// --- 数据包 (Data) 相关 ---

// 数据包头部的大小 (u32: 4 + u16: 2 + u16: 2 + u8: 1 = 9 bytes)
pub const DATA_HEADER_SIZE: usize = 9;
pub const MAX_PAYLOAD_SIZE: usize = 1400;

/// 数据包的头部结构。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataHeader {
    pub frame_id: u32,
    pub packet_id: u16,
    pub total_packets: u16,
    pub is_key_frame: u8,
}

impl DataHeader {
    pub fn to_bytes(&self) -> [u8; DATA_HEADER_SIZE] {
        let mut bytes = [0u8; DATA_HEADER_SIZE];
        bytes[0..4].copy_from_slice(&self.frame_id.to_be_bytes());
        bytes[4..6].copy_from_slice(&self.packet_id.to_be_bytes());
        bytes[6..8].copy_from_slice(&self.total_packets.to_be_bytes());
        bytes[8] = self.is_key_frame;
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < DATA_HEADER_SIZE {
            return None;
        }
        let frame_id = u32::from_be_bytes(bytes[0..4].try_into().ok()?);
        let packet_id = u16::from_be_bytes(bytes[4..6].try_into().ok()?);
        let total_packets = u16::from_be_bytes(bytes[6..8].try_into().ok()?);
        let is_key_frame = bytes[8];
        Some(DataHeader {
            frame_id,
            packet_id,
            total_packets,
            is_key_frame,
        })
    }
}

// --- 确认包 (ACK) 相关 ---
pub const ACK_PACKET_SIZE: usize = size_of::<u32>();

/// 确认包的结构。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AckPacket {
    pub frame_id: u32,
}

impl AckPacket {
    pub fn to_bytes(&self) -> [u8; ACK_PACKET_SIZE] {
        self.frame_id.to_be_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < ACK_PACKET_SIZE {
            return None;
        }
        let frame_id = u32::from_be_bytes(bytes[0..4].try_into().ok()?);
        Some(AckPacket { frame_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_header_serialization() {
        let header = DataHeader {
            frame_id: 12345,
            packet_id: 1,
            total_packets: 10,
            is_key_frame: 1,
        };
        let bytes = header.to_bytes();
        let reconstructed = DataHeader::from_bytes(&bytes).unwrap();
        assert_eq!(header, reconstructed);
    }

    #[test]
    fn test_ack_packet_serialization() {
        let ack = AckPacket { frame_id: 54321 };
        let bytes = ack.to_bytes();
        let reconstructed = AckPacket::from_bytes(&bytes).unwrap();
        assert_eq!(ack, reconstructed);
    }
}
