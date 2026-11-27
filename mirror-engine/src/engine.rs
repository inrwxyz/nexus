use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::adb::{AdbClient, DeviceInfo, WirelessConfig, WirelessManager};
use crate::protocol::{
    AudioPacket, AudioStreamReader, ControlMessage, MirrorServerManager, ServerParams, VideoPacket,
    VideoSessionInfo, VideoStreamReader,
};

pub struct ActiveSession {
    pub device_name: String,
    pub session_info: VideoSessionInfo,
    pub audio_codec: Option<String>,
    pub packet_rx: mpsc::Receiver<VideoPacket>,
    pub audio_rx: Option<mpsc::Receiver<AudioPacket>>,
    control_tx: mpsc::Sender<ControlMessage>,
    adb: AdbClient,
    local_port: u16,
    serial: Option<String>,
    child: tokio::process::Child,
    worker_handle: JoinHandle<()>,
    audio_handle: Option<JoinHandle<()>>,
    control_handle: Option<JoinHandle<()>>,
    _stderr_handle: Option<JoinHandle<()>>,
    cleaned_up: bool,
}

impl ActiveSession {
    pub async fn send_control(&self, msg: ControlMessage) -> Result<()> {
        self.control_tx
            .send(msg)
            .await
            .context("Failed to send control message to queue")?;
        Ok(())
    }

    pub fn try_send_control(&self, msg: ControlMessage) {
        let _ = self.control_tx.try_send(msg);
    }

    pub fn control_sender(&self) -> mpsc::Sender<ControlMessage> {
        self.control_tx.clone()
    }

    /// Cleanly stop the device server, abort stream readers, and remove ADB forward tunnel.
    pub async fn shutdown(&mut self) {
        if self.cleaned_up {
            return;
        }
        self.cleaned_up = true;
        self.worker_handle.abort();
        if let Some(h) = self.audio_handle.take() {
            h.abort();
        }
        if let Some(h) = self.control_handle.take() {
            h.abort();
        }
        let _ = self.child.start_kill();
        let _ = self
            .adb
            .remove_forward(self.serial.as_deref(), self.local_port)
            .await;
    }
}

impl Drop for ActiveSession {
    fn drop(&mut self) {
        if self.cleaned_up {
            return;
        }
        self.cleaned_up = true;
        self.worker_handle.abort();
        if let Some(h) = self.audio_handle.take() {
            h.abort();
        }
        if let Some(h) = self.control_handle.take() {
            h.abort();
        }
        let _ = self.child.start_kill();
        let adb = self.adb.clone();
        let serial = self.serial.clone();
        let port = self.local_port;
        std::thread::spawn(move || {
            let mut cmd = std::process::Command::new(adb.get_adb_path());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000);
            }
            if let Some(s) = serial.as_deref() {
                cmd.args(["-s", s]);
            }
            cmd.args(["forward", "--remove", &format!("tcp:{}", port)]);
            let _ = cmd.output();
        });
    }
}

pub struct MirrorEngine {
    adb: AdbClient,
    wireless: WirelessManager,
    server_mgr: Option<MirrorServerManager>,
}

impl MirrorEngine {
    pub fn new() -> Result<Self> {
        let adb = AdbClient::new()?;
        let wireless = WirelessManager::new(adb.clone());
        let server_mgr = MirrorServerManager::new(adb.clone()).ok();

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

    /// Start native Rust ADB screen mirroring session.
    pub async fn start_mirroring(
        &self,
        serial: Option<&str>,
        params: Option<ServerParams>,
    ) -> Result<ActiveSession> {
        let params = params.unwrap_or_default();
        let session = self
            .server_mgr
            .as_ref()
            .context("Mirror server binary not found")?
            .start(serial, &params)
            .await?;

        // Drain server stdout & stderr so pipes never block and errors are visible.
        let (err_tx, err_rx) = tokio::sync::oneshot::channel::<String>();
        let mut child = session.child;
        let out_pipe = child.stdout.take();
        let err_pipe = child.stderr.take();
        let stderr_handle = Some(tokio::spawn(async move {
            let mut combined = String::new();
            if let Some(mut so) = out_pipe {
                let _ = so.read_to_string(&mut combined).await;
            }
            if let Some(mut se) = err_pipe {
                let _ = se.read_to_string(&mut combined).await;
            }
            let _ = err_tx.send(combined);
        }));

        // 1. Initialize Video Stream Reader (4B codec + 4B width + 4B height = 12B).
        // Pass whole `TcpStream` (never drop a split write-half, which sends TCP FIN on Windows).
        let mut stream_reader = VideoStreamReader::new(session.video_socket);
        let session_info = match stream_reader.read_session_header().await {
            Ok(info) => info,
            Err(e) => {
                let status = child.try_wait();
                let logs = tokio::time::timeout(std::time::Duration::from_millis(800), err_rx)
                    .await
                    .ok()
                    .and_then(|r| r.ok())
                    .unwrap_or_default();
                let _ = child.start_kill();
                let _ = self.adb.remove_forward(session.serial.as_deref(), session.local_port).await;
                return Err(e).context(format!(
                    "Video header failed (dev='{}', status={:?}, logs='{}')",
                    session.device_name,
                    status,
                    logs.trim()
                ));
            }
        };
        log::info!(
            "Video stream initialized: Codec = {}, Resolution = {}x{}",
            session_info.codec,
            session_info.width,
            session_info.height
        );

        let (packet_tx, packet_rx) = mpsc::channel::<VideoPacket>(120);
        let worker_handle = tokio::spawn(async move {
            loop {
                match stream_reader.read_packet().await {
                    Ok(packet) => {
                        if packet_tx.send(packet).await.is_err() {
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

        // 2. Optional Audio Stream Reader (if enabled and supported on device).
        let mut audio_codec = None;
        let mut audio_rx = None;
        let mut audio_handle = None;
        if let Some(audio_sock) = session.audio_socket {
            let mut areader = AudioStreamReader::new(audio_sock);
            if let Ok(Some(codec)) = areader.read_header().await {
                let (atx, arx) = mpsc::channel::<AudioPacket>(128);
                audio_codec = Some(codec);
                audio_rx = Some(arx);
                audio_handle = Some(tokio::spawn(async move {
                    while let Ok(pkt) = areader.read_packet().await {
                        if atx.send(pkt).await.is_err() {
                            break;
                        }
                    }
                }));
            }
        }

        // 3. Control channel
        let (control_tx, mut control_rx) = mpsc::channel::<ControlMessage>(64);
        let control_handle = session.control_socket.map(|mut ctrl_sock| {
            tokio::spawn(async move {
                while let Some(msg) = control_rx.recv().await {
                    let bytes = msg.serialize();
                    if let Err(e) = ctrl_sock.write_all(&bytes).await {
                        log::error!("Failed to write control message: {}", e);
                        break;
                    }
                }
            })
        });

        Ok(ActiveSession {
            device_name: session.device_name,
            session_info,
            audio_codec,
            packet_rx,
            audio_rx,
            control_tx,
            adb: self.adb.clone(),
            local_port: session.local_port,
            serial: session.serial,
            child,
            worker_handle,
            audio_handle,
            control_handle,
            _stderr_handle: stderr_handle,
            cleaned_up: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires a live ADB-connected Android device"]
    async fn live_device_stream_handshake() {
        let engine = MirrorEngine::new().expect("engine init");
        let devs = engine.list_devices().await.expect("list_devices");
        let target = devs
            .into_iter()
            .find(|d| d.state == "device")
            .expect("no connected device");
        let mut session = engine
            .start_mirroring(Some(&target.serial), None)
            .await
            .expect("start_mirroring");
        println!(
            "CONNECTED device={} codec={} {}x{} audio={:?}",
            session.device_name,
            session.session_info.codec,
            session.session_info.width,
            session.session_info.height,
            session.audio_codec
        );
        let p1 = tokio::time::timeout(std::time::Duration::from_secs(5), session.packet_rx.recv())
            .await
            .expect("packet 1 timeout")
            .expect("packet 1 none");
        println!(
            "PKT1 len={} config={} key={} pts={}",
            p1.data.len(),
            p1.is_config,
            p1.is_key_frame,
            p1.pts
        );
        let p2 = tokio::time::timeout(std::time::Duration::from_secs(5), session.packet_rx.recv())
            .await
            .expect("packet 2 timeout")
            .expect("packet 2 none");
        println!(
            "PKT2 len={} config={} key={} pts={}",
            p2.data.len(),
            p2.is_config,
            p2.is_key_frame,
            p2.pts
        );
        session.shutdown().await;
    }
}
