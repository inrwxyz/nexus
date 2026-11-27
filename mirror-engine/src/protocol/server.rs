use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::time::sleep;

use crate::adb::AdbClient;
use crate::protocol::constants::*;

#[derive(Debug, Clone)]
pub struct ServerParams {
    pub max_size: u16,
    pub bit_rate: u32,
    pub max_fps: u8,
    pub audio: bool,
    pub audio_codec: String,
    pub audio_bit_rate: u32,
    pub audio_dup: bool,
    pub control: bool,
    pub stay_awake: bool,
    pub power_off_on_close: bool,
}

impl Default for ServerParams {
    fn default() -> Self {
        Self {
            max_size: DEFAULT_MAX_SIZE,
            bit_rate: DEFAULT_BIT_RATE,
            max_fps: DEFAULT_MAX_FPS,
            audio: true,
            audio_codec: "opus".to_string(),
            audio_bit_rate: 192_000,
            audio_dup: false,
            control: true,
            stay_awake: false,
            power_off_on_close: false,
        }
    }
}

pub struct ServerSession {
    pub device_name: String,
    pub video_socket: TcpStream,
    pub audio_socket: Option<TcpStream>,
    pub control_socket: Option<TcpStream>,
    pub child: tokio::process::Child,
    pub local_port: u16,
    pub serial: Option<String>,
}

pub struct MirrorServerManager {
    adb: AdbClient,
    server_path: PathBuf,
}

impl MirrorServerManager {
    pub fn new(adb: AdbClient) -> Result<Self> {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        let subs = [
            "",
            "assets",
            "resources",
            "resources/assets",
            "_up_/assets",
            "resources/_up_/assets",
        ];
        let names = ["nexus-mirror-server", "nexus-mirror-server.jar"];
        for a in exe_dir.ancestors().take(5) {
            for s in subs {
                for n in names {
                    let p = a.join(s).join(n);
                    if p.is_file() {
                        return Ok(Self { adb, server_path: p });
                    }
                }
            }
        }

        let mut candidates = vec![
            PathBuf::from("assets/nexus-mirror-server"),
            PathBuf::from("assets/nexus-mirror-server.jar"),
            PathBuf::from("../assets/nexus-mirror-server"),
            PathBuf::from("../assets/nexus-mirror-server.jar"),
            exe_dir.join("_up_").join("assets").join("nexus-mirror-server"),
            exe_dir.join("assets").join("nexus-mirror-server"),
            exe_dir.join("nexus-mirror-server"),
        ];

        if let Some(target_dir) = exe_dir.parent() {
            if let Some(root_dir) = target_dir.parent() {
                candidates.push(root_dir.join("assets/nexus-mirror-server"));
            }
        }

        for c in &candidates {
            if c.exists() {
                return Ok(Self {
                    adb,
                    server_path: c.clone(),
                });
            }
        }

        bail!("Could not find mirror device server in assets/")
    }

    /// Push server payload, set up ADB forward tunnel, launch `app_process`, and connect
    /// Video (+ optional Audio + optional Control) sockets in mirror protocol order.
    pub async fn start(&self, serial: Option<&str>, params: &ServerParams) -> Result<ServerSession> {
        let scid: u32 = rand::random::<u32>() & 0x7FFF_FFFF;
        let scid_hex = format!("{:08x}", scid);
        let socket_name = format!("scrcpy_{}", scid_hex);

        log::info!("Pushing mirror device server to device: {:?}...", serial);
        self.adb.push(serial, &self.server_path, DEVICE_SERVER_PATH).await?;

        let local_port = 27183 + (scid % 1000) as u16;
        log::info!(
            "Setting up ADB forward: tcp:{} -> localabstract:{}",
            local_port,
            socket_name
        );
        self.adb.forward(serial, local_port, &socket_name).await?;

        let mut args = vec![
            MIRROR_SERVER_VERSION.to_string(),
            format!("scid={}", scid_hex),
            "log_level=info".to_string(),
            format!("video_bit_rate={}", params.bit_rate),
            format!("max_size={}", params.max_size),
            format!("max_fps={}", params.max_fps),
            "video_codec=h264".to_string(),
            format!("audio={}", params.audio),
            format!("control={}", params.control),
            "tunnel_forward=true".to_string(),
            "cleanup=true".to_string(),
        ];

        if params.audio {
            args.push(format!("audio_codec={}", params.audio_codec));
            args.push(format!("audio_bit_rate={}", params.audio_bit_rate));
            if params.audio_dup {
                args.push("audio_dup=true".to_string());
            }
        }
        if params.stay_awake {
            args.push("stay_awake=true".to_string());
        }
        if params.power_off_on_close {
            args.push("power_off_on_close=true".to_string());
        }

        log::info!("Executing mirror server on device with arguments: {:?}", args);
        let mut child = self.adb.spawn_server_process(serial, DEVICE_SERVER_PATH, &args)?;

        let addr = format!("127.0.0.1:{}", local_port);
        let mut video_socket = None;

        // `adb forward` accepts TCP immediately on PC even before the device server binds its
        // `localabstract` socket on Android, then closes on read until ready. Retrying ACK is required.
        for attempt in 1..=60 {
            if let Ok(Some(status)) = child.try_wait() {
                let mut err_buf = Vec::new();
                if let Some(mut se) = child.stderr.take() {
                    let _ = se.read_to_end(&mut err_buf).await;
                }
                if let Some(mut so) = child.stdout.take() {
                    let _ = so.read_to_end(&mut err_buf).await;
                }
                let _ = self.adb.remove_forward(serial, local_port).await;
                let msg_txt = String::from_utf8_lossy(&err_buf);
                bail!(
                    "Mirror server exited early ({:?}): {}",
                    status.code(),
                    msg_txt.lines().last().unwrap_or("unknown device error")
                );
            }
            sleep(Duration::from_millis(80)).await;
            if let Ok(mut sock) = TcpStream::connect(&addr).await {
                let mut dummy = [0u8; 1];
                match tokio::time::timeout(Duration::from_secs(3), sock.read_exact(&mut dummy)).await
                {
                    Ok(Ok(_)) => {
                        log::info!("Connected and ACKed video stream socket on attempt {}", attempt);
                        video_socket = Some(sock);
                        break;
                    }
                    Ok(Err(_)) | Err(_) => continue,
                }
            }
        }

        let mut video_socket = match video_socket {
            Some(s) => s,
            None => {
                let _ = child.start_kill();
                let _ = self.adb.remove_forward(serial, local_port).await;
                bail!("Timed out waiting for device mirror video socket connection");
            }
        };

        // Remaining enabled sockets (audio, then control) MUST connect BEFORE the device server
        // finishes `DesktopConnection.open()` and writes the 64-byte device name on `video_socket`.
        let audio_socket = if params.audio {
            match TcpStream::connect(&addr).await {
                Ok(s) => Some(s),
                Err(e) => {
                    let _ = child.start_kill();
                    let _ = self.adb.remove_forward(serial, local_port).await;
                    return Err(e).context("Failed to connect to mirror audio socket");
                }
            }
        } else {
            None
        };

        let control_socket = if params.control {
            match TcpStream::connect(&addr).await {
                Ok(s) => Some(s),
                Err(e) => {
                    let _ = child.start_kill();
                    let _ = self.adb.remove_forward(serial, local_port).await;
                    return Err(e).context("Failed to connect to mirror control socket");
                }
            }
        } else {
            None
        };

        // 64-byte device name arrives on the first socket once all sockets are accepted.
        let mut dev_name_buf = [0u8; 64];
        match tokio::time::timeout(
            Duration::from_secs(5),
            video_socket.read_exact(&mut dev_name_buf),
        )
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                let _ = child.start_kill();
                let _ = self.adb.remove_forward(serial, local_port).await;
                return Err(e).context("Failed to read device name");
            }
            Err(_) => {
                let _ = child.start_kill();
                let _ = self.adb.remove_forward(serial, local_port).await;
                bail!("Timed out reading device name from mirror stream");
            }
        }
        let nul_pos = dev_name_buf.iter().position(|&b| b == 0).unwrap_or(64);
        let device_name = String::from_utf8_lossy(&dev_name_buf[..nul_pos]).to_string();
        log::info!("Device Handshake complete! Device Name: {}", device_name);

        Ok(ServerSession {
            device_name,
            video_socket,
            audio_socket,
            control_socket,
            child,
            local_port,
            serial: serial.map(|s| s.to_string()),
        })
    }
}
