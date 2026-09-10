//! Windows System Tunnel (TUN mode).
//!
//! When enabled, tun2socks creates a Wintun adapter (`aether-tun`) and bridges
//! TUN traffic to the Aether SOCKS5 proxy. Windows routing sends all system
//! traffic through the TUN.

use std::sync::atomic::{AtomicBool, Ordering};

static TUN_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn is_active() -> bool {
    TUN_ACTIVE.load(Ordering::Relaxed)
}

pub fn is_admin() -> bool {
    #[cfg(target_os = "windows")]
    {
        platform::is_running_as_admin()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::net::Ipv4Addr;
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::io::Read;

    const TUN_IP: &str = "10.0.0.2";
    const TUN_MASK: &str = "255.255.255.0";
    const TUN_GW: &str = "10.0.0.1";
    const TUN_DNS: &str = "1.1.1.1";
    const TUN_ADAPTER_NAME: &str = "aether-tun";
    const TUN_POLL_ATTEMPTS: u32 = 15;

    pub struct TunManager {
        tun2socks: Option<Child>,
        original_gw: Option<String>,
        tun2socks_path: PathBuf,
    }

    /// Robust admin check using PowerShell (works on all Windows versions).
    pub fn is_running_as_admin() -> bool {
        let output = Command::new("powershell")
            .args([
                "-NoProfile", "-Command",
                "([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)"
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();
        match output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout).trim().to_lowercase();
                log::info!("[tun] Admin check result: {stdout}");
                stdout == "true"
            }
            Err(_) => false,
        }
    }

    impl TunManager {
        pub fn new(resource_dir: &PathBuf) -> Self {
            Self {
                tun2socks: None,
                original_gw: None,
                tun2socks_path: resource_dir.join("binaries").join("tun2socks.exe"),
            }
        }

        pub fn start(&mut self, socks_addr: &str) -> Result<(), String> {
            if !is_running_as_admin() {
                return Err(
                    "System Tunnel requires Administrator privileges. \
                     Right-click the app and select 'Run as administrator'."
                        .into(),
                );
            }

            if !self.tun2socks_path.exists() {
                return Err(format!(
                    "tun2socks.exe not found at {}",
                    self.tun2socks_path.display()
                ));
            }

            log::info!("[tun] Starting tun2socks: {}", self.tun2socks_path.display());
            log::info!("[tun] Proxy: socks5://{}", socks_addr);

            // 1. Launch tun2socks.
            let mut child = Command::new(&self.tun2socks_path)
                .args([
                    "-device", &format!("wintun://{}", TUN_ADAPTER_NAME),
                    "-proxy", &format!("socks5://{}", socks_addr),
                    "-loglevel", "debug",
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| {
                    let msg = format!("Failed to start tun2socks: {e}");
                    log::error!("[tun] {msg}");
                    msg
                })?;

            // 2. Brief check: if tun2socks exits immediately, it failed.
            std::thread::sleep(std::time::Duration::from_secs(1));
            if let Some(exit) = child.try_wait().ok().flatten() {
                // Read stderr for error details.
                let mut stderr = String::new();
                if let Some(mut stderr_pipe) = child.stderr.take() {
                    let _ = stderr_pipe.read_to_string(&mut stderr);
                }
                let msg = format!(
                    "tun2socks exited immediately with {exit}. stderr: {stderr}"
                );
                log::error!("[tun] {msg}");
                return Err(msg);
            }

            self.tun2socks = Some(child);

            // 3. Poll for TUN adapter.
            log::info!("[tun] Polling for TUN adapter '{}'...", TUN_ADAPTER_NAME);
            let mut found = false;
            for attempt in 1..=TUN_POLL_ATTEMPTS {
                std::thread::sleep(std::time::Duration::from_secs(1));
                if adapter_exists(TUN_ADAPTER_NAME) {
                    log::info!("[tun] TUN adapter appeared after {}s", attempt);
                    found = true;
                    break;
                }
                log::info!("[tun] Poll attempt {attempt}/{}", TUN_POLL_ATTEMPTS);
            }
            if !found {
                // Read any tun2socks output before killing.
                let mut stderr = String::new();
                if let Some(ref mut child) = self.tun2socks {
                    if let Some(mut pipe) = child.stderr.take() {
                        let _ = pipe.read_to_string(&mut stderr);
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                }
                self.tun2socks = None;
                let msg = format!(
                    "TUN adapter '{}' did not appear after {}s. \
                     tun2socks stderr: {stderr}",
                    TUN_ADAPTER_NAME, TUN_POLL_ATTEMPTS
                );
                log::error!("[tun] {msg}");
                return Err(msg);
            }

            // 4. Configure TUN adapter.
            run_cmd(&format!(
                "netsh interface ip set address \"{TUN_ADAPTER_NAME}\" static {TUN_IP} {TUN_MASK} {TUN_GW}"
            ))?;
            run_cmd(&format!(
                "netsh interface ip set dns \"{TUN_ADAPTER_NAME}\" static {TUN_DNS}"
            ))?;

            // 5. Capture original gateway.
            self.original_gw = capture_default_gateway();
            log::info!("[tun] Original gateway: {}", self.original_gw.as_deref().unwrap_or("(none)"));

            // 6. Add static route for original gateway (prevent routing loop).
            if let Some(ref gw) = self.original_gw {
                let _ = run_cmd(&format!(
                    "route add {gw} mask 255.255.255.255 {gw} metric 1"
                ));
            }

            // 7. Set TUN as default gateway.
            if let Some(ref gw) = self.original_gw {
                run_cmd(&format!("route delete 0.0.0.0 mask 0.0.0.0 {gw}"))?;
            }
            run_cmd("route add 0.0.0.0 mask 0.0.0.0 10.0.0.1 metric 5")?;

            TUN_ACTIVE.store(true, Ordering::Relaxed);
            log::info!("[tun] System tunnel started successfully");
            Ok(())
        }

        pub fn stop(&mut self) {
            TUN_ACTIVE.store(false, Ordering::Relaxed);

            if let Some(ref gw) = self.original_gw {
                let _ = run_cmd("route delete 0.0.0.0 mask 0.0.0.0 10.0.0.1");
                let _ = run_cmd(&format!("route add 0.0.0.0 mask 0.0.0.0 {gw} metric 10"));
                let _ = run_cmd(&format!("route delete {gw} mask 255.255.255.255 {gw}"));
            }
            self.original_gw = None;

            if let Some(mut child) = self.tun2socks.take() {
                let _ = child.kill();
                let _ = child.wait();
            }

            let _ = run_cmd(&format!(
                "netsh interface ip set dns \"{TUN_ADAPTER_NAME}\" dhcp"
            ));
        }
    }

    impl Drop for TunManager {
        fn drop(&mut self) { self.stop(); }
    }

    fn adapter_exists(name: &str) -> bool {
        let output = match Command::new("netsh")
            .args(["interface", "show", "interface"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
        {
            Ok(o) => o,
            Err(_) => return false,
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let name_lower = name.to_lowercase();
        stdout.lines().any(|l| l.to_lowercase().contains(&name_lower))
    }

    fn capture_default_gateway() -> Option<String> {
        let output = Command::new("route")
            .args(["print", "0.0.0.0"])
            .output()
            .ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.trim().split_whitespace().collect();
            if parts.len() >= 3
                && parts[0] == "0.0.0.0"
                && parts[1] == "0.0.0.0"
                && parts[2] != "0.0.0.0"
                && parts[2].parse::<Ipv4Addr>().is_ok()
            {
                return Some(parts[2].to_string());
            }
        }
        None
    }

    fn run_cmd(cmd: &str) -> Result<(), String> {
        log::info!("[tun] Running: {cmd}");
        let output = Command::new("cmd")
            .args(["/C", cmd])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("Failed to run `{cmd}`: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let msg = if stderr.trim().is_empty() {
                format!("Command failed: {cmd}")
            } else {
                format!("Command failed: {cmd}\nstderr: {stderr}")
            };
            log::error!("[tun] {msg}");
            if cmd.starts_with("netsh") || cmd.starts_with("route") {
                return Err(format!("{msg}\nRun as Administrator required"));
            }
            return Err(msg);
        }
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use std::path::PathBuf;
    pub struct TunManager;
    impl TunManager {
        pub fn new(_resource_dir: &PathBuf) -> Self { Self }
        pub fn start(&mut self, _socks_addr: &str) -> Result<(), String> {
            Err("System tunnel is only supported on Windows".into())
        }
        pub fn stop(&mut self) {}
    }
    impl Drop for TunManager {
        fn drop(&mut self) { self.stop(); }
    }
    pub fn is_running_as_admin() -> bool { false }
}

pub use platform::TunManager;

pub fn new_manager(resource_dir: &std::path::Path) -> TunManager {
    TunManager::new(&resource_dir.to_path_buf())
}
