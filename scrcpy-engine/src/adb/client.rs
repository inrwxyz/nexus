use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub serial: String,
    pub state: String,
    pub model: String,
    pub is_wireless: bool,
}

#[derive(Clone, Debug)]
pub struct AdbClient {
    adb_path: PathBuf,
}

impl AdbClient {
    /// Discovers `adb` executable from system PATH or bundled assets
    pub fn new() -> Result<Self> {
        // 1. First preference: Check if system `adb` is available in PATH.
        // Using the user's system adb directly ensures:
        // - Zero daemon version conflicts between user's terminal/CMD and Nexus
        // - Nexus never locks or conflicts with bundled assets/adb.exe
        // - Existing user wireless/USB connections are shared seamlessly
        let mut test_cmd = std::process::Command::new("adb");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            test_cmd.creation_flags(0x08000000);
        }
        if test_cmd.arg("version").output().is_ok() {
            log::info!("Using system adb found in PATH");
            return Ok(Self { adb_path: PathBuf::from("adb") });
        }

        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        // Comprehensive candidate search paths for both Installed (NSIS/MSI) and Portable runs
        let mut candidates = vec![
            exe_dir.join("adb.exe"),
            exe_dir.join("assets").join("adb.exe"),
            exe_dir.join("resources").join("adb.exe"),
            exe_dir.join("resources").join("assets").join("adb.exe"),
            exe_dir.join("_up_").join("assets").join("adb.exe"),
            exe_dir.join("resources").join("_up_").join("assets").join("adb.exe"),
            PathBuf::from("assets/adb.exe"),
            PathBuf::from("resources/assets/adb.exe"),
            PathBuf::from("resources/adb.exe"),
            PathBuf::from("adb.exe"),
        ];

        if let Some(parent) = exe_dir.parent() {
            candidates.push(parent.join("assets").join("adb.exe"));
            candidates.push(parent.join("adb.exe"));
            if let Some(grandparent) = parent.parent() {
                candidates.push(grandparent.join("assets").join("adb.exe"));
            }
        }

        for c in &candidates {
            if c.exists() {
                log::info!("Discovered adb executable at: {}", c.display());
                return Ok(Self { adb_path: c.clone() });
            }
        }

        // Deep search within exe_dir subdirectories (up to depth 2)
        if let Ok(entries) = std::fs::read_dir(&exe_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    let direct = p.join("adb.exe");
                    if direct.exists() {
                        log::info!("Discovered adb executable via subfolder search: {}", direct.display());
                        return Ok(Self { adb_path: direct });
                    }
                    if let Ok(sub_entries) = std::fs::read_dir(&p) {
                        for sub in sub_entries.flatten() {
                            let sp = sub.path();
                            if sp.is_dir() {
                                let sub_direct = sp.join("adb.exe");
                                if sub_direct.exists() {
                                    log::info!("Discovered adb executable via nested search: {}", sub_direct.display());
                                    return Ok(Self { adb_path: sub_direct });
                                }
                            }
                        }
                    }
                }
            }
        }

        bail!("Could not find adb executable. Please ensure Provided by Nexus is installed properly.")
    }

    pub fn get_adb_path(&self) -> &Path {
        &self.adb_path
    }

    /// Creates a tokio Command configured with CREATE_NO_WINDOW on Windows to prevent terminal popups
    pub fn new_command(&self) -> Command {
        let mut cmd = Command::new(&self.adb_path);
        #[cfg(windows)]
        cmd.creation_flags(0x08000000);

        // Run from the directory where adb.exe lives so AdbWinApi.dll & AdbWinUsbApi.dll are found
        if let Some(parent) = self.adb_path.parent() {
            if parent.is_dir() {
                cmd.current_dir(parent);
            }
        }
        cmd
    }

    /// List connected devices
    pub async fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        let output = self.new_command()
            .args(["devices", "-l"])
            .output()
            .await
            .context("Failed to run `adb devices -l`")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut devices = Vec::new();

        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("List of devices") || line.starts_with('*') {
                continue;
            }

            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let serial = parts[0].to_string();
                let state = parts[1].to_string();

                let mut model = "Unknown".to_string();
                for p in &parts[2..] {
                    if let Some(m) = p.strip_prefix("model:") {
                        model = m.to_string();
                        break;
                    }
                }

                let is_wireless = serial.contains(':') || serial.starts_with("adb-");

                devices.push(DeviceInfo {
                    serial,
                    state,
                    model,
                    is_wireless,
                });
            }
        }

        Ok(devices)
    }

    /// Push file to device
    pub async fn push(&self, serial: Option<&str>, local: &Path, remote: &str) -> Result<()> {
        let mut cmd = self.new_command();
        if let Some(s) = serial {
            cmd.args(["-s", s]);
        }
        cmd.args(["push", local.to_string_lossy().as_ref(), remote]);

        let output = cmd.output().await.context("Failed to push file via adb")?;
        if !output.status.success() {
            bail!("ADB push failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(())
    }

    /// Set up port forwarding: tcp:<local_port> -> localabstract:<remote_abstract_socket>
    pub async fn forward(&self, serial: Option<&str>, local_port: u16, remote_abstract: &str) -> Result<()> {
        let mut cmd = self.new_command();
        if let Some(s) = serial {
            cmd.args(["-s", s]);
        }
        let local_spec = format!("tcp:{}", local_port);
        let remote_spec = format!("localabstract:{}", remote_abstract);
        cmd.args(["forward", &local_spec, &remote_spec]);

        let output = cmd.output().await.context("Failed to set up adb forward")?;
        if !output.status.success() {
            bail!("ADB forward failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(())
    }

    /// Remove port forwarding
    pub async fn remove_forward(&self, serial: Option<&str>, local_port: u16) -> Result<()> {
        let mut cmd = self.new_command();
        if let Some(s) = serial {
            cmd.args(["-s", s]);
        }
        cmd.args(["forward", "--remove", &format!("tcp:{}", local_port)]);
        let _ = cmd.output().await;
        Ok(())
    }

    /// Execute a shell command and return stdout
    pub async fn shell(&self, serial: Option<&str>, shell_cmd: &str) -> Result<String> {
        let mut cmd = self.new_command();
        if let Some(s) = serial {
            cmd.args(["-s", s]);
        }
        cmd.args(["shell", shell_cmd]);

        let output = cmd.output().await.context("Failed to execute adb shell")?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Spawns the scrcpy server process on device in the background
    pub fn spawn_server_process(
        &self,
        serial: Option<&str>,
        server_jar: &str,
        args: &[String],
    ) -> Result<tokio::process::Child> {
        let mut cmd = self.new_command();
        if let Some(s) = serial {
            cmd.args(["-s", s]);
        }
        cmd.args(["shell", &format!("CLASSPATH={}", server_jar), "app_process", "/", "com.genymobile.scrcpy.Server"]);
        for a in args {
            cmd.arg(a);
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let child = cmd.spawn().context("Failed to spawn scrcpy-server via adb shell")?;
        Ok(child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_devices() {
        if let Ok(client) = AdbClient::new() {
            println!("ADB path: {:?}", client.get_adb_path());
            let devs = client.list_devices().await;
            println!("Devices: {:?}", devs);
        }
    }
}
