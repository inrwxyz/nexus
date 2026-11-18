use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;
use crate::adb::client::AdbClient;

const DEFAULT_WIRELESS_PORT: u16 = 5555;
const CONFIG_FILE: &str = "wireless_device.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WirelessConfig {
    pub ip: String,
    pub port: u16,
    pub model: String,
    pub auto_reconnect: bool,
    pub last_connected_at: String,
}

pub struct WirelessManager {
    client: AdbClient,
    config_path: PathBuf,
}

impl WirelessManager {
    pub fn new(client: AdbClient) -> Self {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        let config_path = if PathBuf::from(CONFIG_FILE).exists() {
            PathBuf::from(CONFIG_FILE)
        } else if exe_dir.join(CONFIG_FILE).exists() {
            exe_dir.join(CONFIG_FILE)
        } else {
            // Default to current directory if not existing yet
            PathBuf::from(CONFIG_FILE)
        };

        Self {
            client,
            config_path,
        }
    }

    /// Automatically pairs and locks ADB over TCP/IP 5555, extracts Phone Wi-Fi IP, and connects.
    /// Works seamlessly for both USB-connected phones and Wi-Fi phones on dynamic ports (e.g. 35659).
    pub async fn setup_permanent_wireless(&self, serial: &str) -> Result<WirelessConfig> {
        let serial = serial.trim();
        log::info!("Initiating Permanent Wireless ADB setup for device: {}", serial);

        let is_wireless = serial.contains(':');

        // Step 1: Determine the target phone Wi-Fi IP address
        let ip = if is_wireless {
            // Already connected via wireless IP:port (e.g. 192.168.1.50:5555)
            let ip_part = serial.split(':').next().unwrap_or("").trim();
            if ip_part.is_empty() {
                bail!("Invalid wireless device serial: {}", serial);
            }
            ip_part.to_string()
        } else {
            // Connected via USB cable (e.g. RE608EL1) - query IP while USB connection is active
            self.discover_device_ip(serial).await.with_context(|| {
                format!("Could not determine Phone Wi-Fi IP from USB device '{}'. Ensure phone is connected to Wi-Fi.", serial)
            })?
        };

        log::info!("Target phone Wi-Fi IP: {}", ip);

        // If the device is already connected on port 5555, simply persist config and return
        if is_wireless && serial.ends_with(&format!(":{}", DEFAULT_WIRELESS_PORT)) {
            let model = self.client.shell(Some(serial), "getprop ro.product.model")
                .await
                .unwrap_or_else(|_| "Android Device".to_string());

            let config = WirelessConfig {
                ip: ip.clone(),
                port: DEFAULT_WIRELESS_PORT,
                model,
                auto_reconnect: true,
                last_connected_at: chrono_timestamp(),
            };
            self.save_config(&config).await?;
            return Ok(config);
        }

        // Step 2: Switch ADB daemon on device to permanent port 5555
        // When switching from a dynamic wireless port, adbd restarts on port 5555 and drops the old port.
        let _ = self.client.new_command()
            .args(["-s", serial, "tcpip", &DEFAULT_WIRELESS_PORT.to_string()])
            .output()
            .await;

        // Clean up old dynamic port if wireless to avoid ghost offline devices
        if is_wireless {
            let _ = self.client.new_command()
                .args(["disconnect", serial])
                .output()
                .await;
        }

        // Wait for Android's adbd daemon to restart and bind to 5555
        tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;

        // Step 3: Connect to the phone on permanent port 5555 (retry up to 4 times)
        let connect_target = format!("{}:{}", ip, DEFAULT_WIRELESS_PORT);
        let mut connected = false;
        let mut last_err = String::new();

        for attempt in 1..=4 {
            log::info!("Connecting to permanent target {} (attempt {}/4)...", connect_target, attempt);
            let connect_out = self.client.new_command()
                .args(["connect", &connect_target])
                .output()
                .await;

            if let Ok(out) = connect_out {
                let res_str = String::from_utf8_lossy(&out.stdout);
                log::info!("ADB connect attempt {} response: {}", attempt, res_str.trim());
                if res_str.contains("connected to") || res_str.contains("already connected") {
                    connected = true;
                    break;
                } else {
                    last_err = res_str.trim().to_string();
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
        }

        if !connected {
            bail!("Failed to connect to phone at {}:5555: {}", ip, last_err);
        }

        // Step 4: Extract model name from the active 5555 connection
        let model = self.client.shell(Some(&connect_target), "getprop ro.product.model")
            .await
            .unwrap_or_else(|_| "Android Device".to_string());

        let config = WirelessConfig {
            ip: ip.clone(),
            port: DEFAULT_WIRELESS_PORT,
            model,
            auto_reconnect: true,
            last_connected_at: chrono_timestamp(),
        };

        // Step 5: Persist config for auto-reconnect
        self.save_config(&config).await?;
        log::info!("Permanent Wireless ADB configuration saved: {}:5555", ip);

        Ok(config)
    }

    /// Tries to query the phone's Wi-Fi IP address. Only ever accepts the `wlan*` interface,
    /// so an active mobile-data link (rmnet_data*, ccmni*, ...) can never be returned by mistake.
    pub async fn discover_device_ip(&self, serial: &str) -> Result<String> {
        // Method 1 (most reliable): ask the Wi-Fi interface directly
        for iface in ["wlan0", "wlan1"] {
            let cmd = format!("ip -f inet addr show {}", iface);
            if let Ok(out) = self.client.shell(Some(serial), &cmd).await {
                for line in out.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("inet ") {
                        let parts: Vec<&str> = trimmed.split_whitespace().collect();
                        if parts.len() >= 2 {
                            let ip_mask = parts[1];
                            let ip = ip_mask.split('/').next().unwrap_or(ip_mask);
                            if is_ipv4(ip) {
                                return Ok(ip.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Method 2: `ip route`, but only lines that explicitly belong to a wlan interface
        if let Ok(out) = self.client.shell(Some(serial), "ip route").await {
            for line in out.lines() {
                if (line.contains("wlan0") || line.contains("wlan1") || line.contains("dev wlan"))
                    && line.contains("src")
                {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    for (i, p) in parts.iter().enumerate() {
                        if *p == "src" && i + 1 < parts.len() && is_ipv4(parts[i + 1]) {
                            return Ok(parts[i + 1].to_string());
                        }
                    }
                }
            }
        }

        // Method 3: getprop dhcp.wlan0.ipaddress
        if let Ok(out) = self.client.shell(Some(serial), "getprop dhcp.wlan0.ipaddress").await {
            let trimmed = out.trim();
            if is_ipv4(trimmed) {
                return Ok(trimmed.to_string());
            }
        }

        bail!("Could not automatically determine Phone Wi-Fi IP address. Ensure Phone is connected to Wi-Fi.")
    }

    /// Connect directly via IP and Port
    pub async fn connect(&self, ip: &str, port: u16) -> Result<String> {
        let target = format!("{}:{}", ip, port);
        let out = self.client.new_command()
            .args(["connect", &target])
            .output()
            .await?;
        let res = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(res)
    }

    /// Disconnect
    pub async fn disconnect(&self, ip: &str, port: u16) -> Result<String> {
        let target = format!("{}:{}", ip, port);
        let out = self.client.new_command()
            .args(["disconnect", &target])
            .output()
            .await?;
        let res = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(res)
    }

    /// Load saved wireless configuration
    pub async fn load_config(&self) -> Result<WirelessConfig> {
        if !self.config_path.exists() {
            bail!("No previously saved wireless device configuration found. Connect your phone via USB and run with `--pair` first.");
        }
        let content = fs::read_to_string(&self.config_path).await
            .with_context(|| format!("Failed to read {}", self.config_path.display()))?;
        let config: WirelessConfig = serde_json::from_str(&content)
            .context("Invalid wireless configuration JSON")?;
        Ok(config)
    }

    /// Save configuration across candidate paths
    pub async fn save_config(&self, config: &WirelessConfig) -> Result<()> {
        let json = serde_json::to_string_pretty(config)?;
        let _ = fs::write(&self.config_path, &json).await;
        let _ = fs::write(CONFIG_FILE, &json).await;
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let _ = fs::write(parent.join(CONFIG_FILE), &json).await;
            }
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let p = PathBuf::from(local).join("Nexus");
            let _ = fs::create_dir_all(&p).await;
            let _ = fs::write(p.join(CONFIG_FILE), &json).await;
        }
        Ok(())
    }

    /// Reconnect to the last saved wireless device automatically
    pub async fn reconnect_last_saved(&self) -> Result<String> {
        let cfg = self.load_config().await?;
        self.connect(&cfg.ip, cfg.port).await
    }
}

fn chrono_timestamp() -> String {
    // Standard ISO timestamp
    let now = std::time::SystemTime::now();
    format!("{:?}", now)
}

fn is_ipv4(s: &str) -> bool {
    let p: Vec<&str> = s.split('.').collect();
    p.len() == 4 && p.iter().all(|x| !x.is_empty() && x.len() <= 3 && x.parse::<u8>().is_ok())
}
