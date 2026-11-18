use anyhow::{Context, Result};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::adb::{AdbClient, DeviceInfo, WirelessConfig, WirelessManager};
use crate::protocol::{
    ControlMessage, ScrcpyServerManager, ServerParams, VideoPacket, VideoSessionInfo,
    VideoStreamReader,
};

pub struct ActiveSession {
    pub device_name: String,
    pub session_info: VideoSessionInfo,
    pub packet_rx: mpsc::Receiver<VideoPacket>,
    control_tx: mpsc::Sender<ControlMessage>,
    _worker_handle: JoinHandle<()>,
    _control_handle: Option<JoinHandle<()>>,
}

impl ActiveSession {
    pub async fn send_control(&self, msg: ControlMessage) -> Result<()> {
        self.control_tx.send(msg).await.context("Failed to send control message to queue")?;
        Ok(())
    }
}

pub struct ScrcpyEngine {
    adb: AdbClient,
    wireless: WirelessManager,
    server_mgr: Option<ScrcpyServerManager>,
}

impl ScrcpyEngine {
    pub fn new() -> Result<Self> {
        let adb = AdbClient::new()?;
        let wireless = WirelessManager::new(adb.clone());
        let server_mgr = ScrcpyServerManager::new(adb.clone()).ok();

        Ok(Self {
            adb,
            wireless,
            server_mgr,
        })
    }

    /// List all connected devices (USB & Wireless)
    pub async fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        self.adb.list_devices().await
    }

    /// One-click setup: lock ADB to fixed port 5555, discover IP, and connect permanently over Wi-Fi
    pub async fn enable_permanent_wireless(&self, usb_serial: &str) -> Result<WirelessConfig> {
        self.wireless.setup_permanent_wireless(usb_serial).await
    }

    /// Connect directly to a wireless phone using IP and Port
    pub async fn connect_wireless(&self, ip: &str, port: u16) -> Result<String> {
        self.wireless.connect(ip, port).await
    }

    /// Disconnect wireless device
    pub async fn disconnect_wireless(&self, ip: &str, port: u16) -> Result<String> {
        self.wireless.disconnect(ip, port).await
    }

    /// Reconnect to last saved wireless device
    pub async fn reconnect_last_saved(&self) -> Result<String> {
        self.wireless.reconnect_last_saved().await
    }

    /// Start high-performance, low-RAM scrcpy screen mirroring
    pub async fn start_mirroring(
        &self,
        serial: Option<&str>,
        params: Option<ServerParams>,
    ) -> Result<ActiveSession> {
        let params = params.unwrap_or_default();
        let session = self
            .server_mgr
            .as_ref()
            .context("scrcpy-server binary not found")?
            .start(serial, &params)
            .await?;

        // 1. Initialize Video Stream Reader
        let (v_read, _) = session.video_socket.into_split();
        let mut stream_reader = VideoStreamReader::new(v_read);

        log::info!("Negotiating video stream session parameters...");
        let session_info = stream_reader.read_session_header().await?;
        log::info!(
            "Video stream initialized: Codec = {}, Resolution = {}x{}",
            session_info.codec,
            session_info.width,
            session_info.height
        );

        // 2. Channel to emit video packets to UI/decoder
        let (packet_tx, packet_rx) = mpsc::channel::<VideoPacket>(120);

        let worker_handle = tokio::spawn(async move {
            loop {
                match stream_reader.read_packet().await {
                    Ok(packet) => {
                        if packet_tx.send(packet).await.is_err() {
                            log::info!("Packet consumer channel closed. Stopping video reader.");
                            break;
                        }
                    }
                    Err(e) => {
                        log::warn!("Video stream connection ended: {}", e);
                        break;
                    }
                }
            }
        });

        // 3. Control channel
        let (control_tx, mut control_rx) = mpsc::channel::<ControlMessage>(64);
        let control_handle = if let Some(ctrl_sock) = session.control_socket {
            let (_, mut c_write) = ctrl_sock.into_split();
            let handle = tokio::spawn(async move {
                while let Some(msg) = control_rx.recv().await {
                    let bytes = msg.serialize();
                    if let Err(e) = c_write.write_all(&bytes).await {
                        log::error!("Failed to write control message: {}", e);
                        break;
                    }
                }
            });
            Some(handle)
        } else {
            None
        };

        Ok(ActiveSession {
            device_name: session.device_name,
            session_info,
            packet_rx,
            control_tx,
            _worker_handle: worker_handle,
            _control_handle: control_handle,
        })
    }
}
