use anyhow::{bail, Result};
use byteorder::{BigEndian, ByteOrder};
use tokio::io::{AsyncRead, AsyncReadExt};

pub const SC_PACKET_FLAG_CONFIG: u64 = 1 << 63;
pub const SC_PACKET_FLAG_KEY_FRAME: u64 = 1 << 62;
pub const SC_PACKET_PTS_MASK: u64 = !(3 << 62);
const MAX_PACKET_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoSessionInfo {
    pub codec: String, // e.g. "h264"
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct VideoPacket {
    pub pts: u64,
    pub is_config: bool,
    pub is_key_frame: bool,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct AudioPacket {
    pub pts: u64,
    pub is_config: bool,
    pub is_key_frame: bool,
    pub data: Vec<u8>,
}

pub struct VideoStreamReader<R> {
    reader: R,
    pub session_info: Option<VideoSessionInfo>,
}

impl<R: AsyncRead + Unpin> VideoStreamReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            session_info: None,
        }
    }

    /// Reads the 12-byte video codec metadata header (`4B codec_id + 4B width + 4B height`).
    pub async fn read_session_header(&mut self) -> Result<VideoSessionInfo> {
        let mut codec_buf = [0u8; 4];
        self.reader.read_exact(&mut codec_buf).await?;

        let raw_codec_id = BigEndian::read_u32(&codec_buf);
        if raw_codec_id == 0 {
            bail!("Video stream was explicitly disabled by device");
        } else if raw_codec_id == 1 {
            bail!("Video stream configuration error on device");
        }

        let codec_str = String::from_utf8_lossy(&codec_buf)
            .trim_end_matches('\0')
            .to_string();

        let mut dims_buf = [0u8; 8];
        self.reader.read_exact(&mut dims_buf).await?;
        let width = BigEndian::read_u32(&dims_buf[0..4]);
        let height = BigEndian::read_u32(&dims_buf[4..8]);

        let info = VideoSessionInfo {
            codec: codec_str,
            width,
            height,
        };
        self.session_info = Some(info.clone());
        Ok(info)
    }

    /// Reads the next video packet (`12B header + payload`).
    pub async fn read_packet(&mut self) -> Result<VideoPacket> {
        let (pts, is_config, is_key_frame, data) = read_framed_packet(&mut self.reader).await?;
        Ok(VideoPacket {
            pts,
            is_config,
            is_key_frame,
            data,
        })
    }
}

pub struct AudioStreamReader<R> {
    reader: R,
}

impl<R: AsyncRead + Unpin> AudioStreamReader<R> {
    pub fn new(reader: R) -> Self {
        Self { reader }
    }

    /// Reads the 4-byte audio codec ID (`0` = disabled on device, `1` = error -> `Ok(None)`).
    pub async fn read_header(&mut self) -> Result<Option<String>> {
        let mut codec_buf = [0u8; 4];
        self.reader.read_exact(&mut codec_buf).await?;
        let raw_id = BigEndian::read_u32(&codec_buf);
        if raw_id == 0 || raw_id == 1 {
            return Ok(None);
        }
        let codec = String::from_utf8_lossy(&codec_buf)
            .trim_matches('\0')
            .to_string();
        if codec.is_empty() {
            return Ok(None);
        }
        Ok(Some(codec))
    }

    /// Reads the next audio packet (`12B header + payload`).
    pub async fn read_packet(&mut self) -> Result<AudioPacket> {
        let (pts, is_config, is_key_frame, data) = read_framed_packet(&mut self.reader).await?;
        Ok(AudioPacket {
            pts,
            is_config,
            is_key_frame,
            data,
        })
    }
}

async fn read_framed_packet<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<(u64, bool, bool, Vec<u8>)> {
    let mut header = [0u8; 12];
    reader.read_exact(&mut header).await?;

    let pts_flags = BigEndian::read_u64(&header[0..8]);
    let len = BigEndian::read_u32(&header[8..12]) as usize;

    if len == 0 || len > MAX_PACKET_BYTES {
        bail!("Invalid packet length received: {len}");
    }

    let is_config = (pts_flags & SC_PACKET_FLAG_CONFIG) != 0;
    let is_key_frame = (pts_flags & SC_PACKET_FLAG_KEY_FRAME) != 0;
    let pts = pts_flags & SC_PACKET_PTS_MASK;

    let mut data = vec![0u8; len];
    reader.read_exact(&mut data).await?;
    Ok((pts, is_config, is_key_frame, data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[tokio::test]
    async fn video_reader_parses_12_byte_header_and_packet() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"h264");
        bytes.extend_from_slice(&1080u32.to_be_bytes());
        bytes.extend_from_slice(&2400u32.to_be_bytes());

        let pts_flags = SC_PACKET_FLAG_KEY_FRAME | 123_456u64;
        bytes.extend_from_slice(&pts_flags.to_be_bytes());
        bytes.extend_from_slice(&4u32.to_be_bytes());
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);

        let mut reader = VideoStreamReader::new(Cursor::new(bytes));
        let info = reader.read_session_header().await.unwrap();
        assert_eq!(info.codec, "h264");
        assert_eq!(info.width, 1080);
        assert_eq!(info.height, 2400);

        let pkt = reader.read_packet().await.unwrap();
        assert!(!pkt.is_config);
        assert!(pkt.is_key_frame);
        assert_eq!(pkt.pts, 123_456);
        assert_eq!(pkt.data, vec![0x00, 0x00, 0x00, 0x01]);
    }

    #[tokio::test]
    async fn audio_reader_handles_disabled_and_opus_packets() {
        let mut dis = AudioStreamReader::new(Cursor::new(0u32.to_be_bytes().to_vec()));
        assert_eq!(dis.read_header().await.unwrap(), None);

        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"opus");
        bytes.extend_from_slice(&(SC_PACKET_FLAG_CONFIG).to_be_bytes());
        bytes.extend_from_slice(&3u32.to_be_bytes());
        bytes.extend_from_slice(&[0x4f, 0x70, 0x75]);

        let mut reader = AudioStreamReader::new(Cursor::new(bytes));
        assert_eq!(reader.read_header().await.unwrap().as_deref(), Some("opus"));
        let pkt = reader.read_packet().await.unwrap();
        assert!(pkt.is_config);
        assert_eq!(pkt.data, vec![0x4f, 0x70, 0x75]);
    }
}
