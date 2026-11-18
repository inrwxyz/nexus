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
            audio: false,
            control: true,
            stay_awake: true,
            power_off_on_close: false,
        }
    }
}

pub struct ServerSession {
    pub device_name: String,
    pub video_socket: TcpStream,
    pub control_socket: Option<TcpStream>,
    pub child: tokio::process::Child,
    pub local_port: u16,
    pub serial: Option<String>,
}

pub struct ScrcpyServerManager {
    adb: AdbClient,
    server_path: PathBuf,
}

impl ScrcpyServerManager {
    pub fn new(adb: AdbClient) -> Result<Self> {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        let subs = ["", "assets", "resources", "resources/assets", "_up_/assets", "resources/_up_/assets"];
        for a in exe_dir.ancestors().take(5) {
            for s in subs {
                let p = a.join(s).join("scrcpy-server");
                if p.is_file() {
                    return Ok(Self { adb, server_path: p });
                }
                let p_jar = a.join(s).join("scrcpy-server.jar");
                if p_jar.is_file() {
                    return Ok(Self { adb, server_path: p_jar });
                }
            }
        }

        let mut candidates = vec![
            PathBuf::from("assets/scrcpy-server"),
            PathBuf::from("assets/scrcpy-server.jar"),
            exe_dir.join("_up_").join("assets").join("scrcpy-server"),
            exe_dir.join("assets").join("scrcpy-server"),
            exe_dir.join("scrcpy-server"),
        ];

        // Also check if running from target/debug or target/release
        if let Some(target_dir) = exe_dir.parent() {
            if let Some(root_dir) = target_dir.parent() {
                candidates.push(root_dir.join("assets/scrcpy-server"));
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

        bail!("Could not find `scrcpy-server` binary in assets/ or references/")
    }

    /// Push server jar, set up forwarding tunnel, launch app_process, and establish video & control streams
    pub async fn start(&self, serial: Option<&str>, params: &ServerParams) -> Result<ServerSession> {
        let scid: u32 = rand::random::<u32>() & 0x7FFFFFFF;
        let scid_hex = format!("{:08x}", scid);
        let socket_name = format!("scrcpy_{}", scid_hex);

        log::info!("Pushing scrcpy-server jar to device: {:?}...", serial);
        self.adb.push(serial, &self.server_path, DEVICE_SERVER_PATH).await?;

        let local_port = 27183 + (scid % 1000) as u16;
        log::info!("Setting up ADB forward: tcp:{} -> localabstract:{}", local_port, socket_name);
        self.adb.forward(serial, local_port, &socket_name).await?;

        let mut args = vec![
            SCRCPY_VERSION.to_string(),
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

        if params.stay_awake {
            args.push("stay_awake=true".to_string());
        }
        if params.power_off_on_close {
            args.push("power_off_on_close=true".to_string());
        }

        log::info!("Executing server on device with arguments: {:?}", args);
        let child = self.adb.spawn_server_process(serial, DEVICE_SERVER_PATH, &args)?;

        // Connect with retries to local forwarded TCP port
        let addr = format!("127.0.0.1:{}", local_port);
        let mut video_socket = None;

        for attempt in 1..=50 {
            sleep(Duration::from_millis(100)).await;
            match TcpStream::connect(&addr).await {
                Ok(sock) => {
                    log::info!("Connected to video stream socket on attempt {}", attempt);
                    video_socket = Some(sock);
                    break;
                }
                Err(_) => continue,
            }
        }

        let mut video_socket = match video_socket {
            Some(s) => s,
            None => bail!("Timed out waiting for scrcpy-server video socket connection"),
        };

        // 1. Read dummy byte (ACK) from video socket
        let mut dummy = [0u8; 1];
        video_socket.read_exact(&mut dummy).await
            .context("Failed to read dummy byte from video stream")?;

        // 2. Read 64-byte device name
        let mut dev_name_buf = [0u8; 64];
        video_socket.read_exact(&mut dev_name_buf).await
            .context("Failed to read device name")?;
        let nul_pos = dev_name_buf.iter().position(|&b| b == 0).unwrap_or(64);
        let device_name = String::from_utf8_lossy(&dev_name_buf[..nul_pos]).to_string();
        log::info!("Device Handshake complete! Device Name: {}", device_name);

        // 3. Connect control socket if control is enabled
        let mut control_socket = None;
        if params.control {
            let mut ctrl = TcpStream::connect(&addr).await
                .context("Failed to connect to scrcpy-server control socket")?;
            
            // Read dummy byte from control socket
            let mut ctrl_dummy = [0u8; 1];
            ctrl.read_exact(&mut ctrl_dummy).await
                .context("Failed to read dummy byte from control socket")?;

            log::info!("Connected to control socket successfully!");
            control_socket = Some(ctrl);
        }

        Ok(ServerSession {
            device_name,
            video_socket,
            control_socket,
            child,
            local_port,
            serial: serial.map(|s| s.to_string()),
        })
    }
}
