use byteorder::{BigEndian, ByteOrder};
use tokio::io::{AsyncRead, AsyncReadExt};
use anyhow::{Result, bail};

pub const SC_PACKET_FLAG_CONFIG: u64 = 1 << 63;
pub const SC_PACKET_FLAG_KEY_FRAME: u64 = 1 << 62;
pub const SC_PACKET_PTS_MASK: u64 = !(3 << 62);

#[derive(Debug, Clone)]
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

    /// Reads initial codec and session video dimensions (width x height)
    pub async fn read_session_header(&mut self) -> Result<VideoSessionInfo> {
        // 1. Read 4 bytes codec ID
        let mut codec_buf = [0u8; 4];
        self.reader.read_exact(&mut codec_buf).await?;
        let codec_str = String::from_utf8_lossy(&codec_buf).to_string();

        let raw_codec_id = BigEndian::read_u32(&codec_buf);
        if raw_codec_id == 0 {
            bail!("Video stream was explicitly disabled by device");
        } else if raw_codec_id == 1 {
            bail!("Video stream configuration error on device");
        }

        // 2. Read 12-byte session header
        let mut session_buf = [0u8; 12];
        self.reader.read_exact(&mut session_buf).await?;
        let width = BigEndian::read_u32(&session_buf[4..8]);
        let height = BigEndian::read_u32(&session_buf[8..12]);

        let info = VideoSessionInfo {
            codec: codec_str,
            width,
            height,
        };
        self.session_info = Some(info.clone());
        Ok(info)
    }

    /// Reads the next video packet (12-byte header + payload)
    pub async fn read_packet(&mut self) -> Result<VideoPacket> {
        let mut header = [0u8; 12];
        self.reader.read_exact(&mut header).await?;

        let pts_flags = BigEndian::read_u64(&header[0..8]);
        let len = BigEndian::read_u32(&header[8..12]) as usize;

        if len == 0 {
            bail!("Invalid 0-length video packet received");
        }

        let is_config = (pts_flags & SC_PACKET_FLAG_CONFIG) != 0;
        let is_key_frame = (pts_flags & SC_PACKET_FLAG_KEY_FRAME) != 0;
        let pts = pts_flags & SC_PACKET_PTS_MASK;

        let mut data = vec![0u8; len];
        self.reader.read_exact(&mut data).await?;

        Ok(VideoPacket {
            pts,
            is_config,
            is_key_frame,
            data,
        })
    }
}
