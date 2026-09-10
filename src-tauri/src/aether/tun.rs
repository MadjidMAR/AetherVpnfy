//! Windows System Tunnel (TUN mode) for hotspot/tethering sharing.
//!
//! When enabled, all system traffic routes through the Aether tunnel via a
//! TUN adapter + tun2socks bridge. Architecture:
//!
//! - Aether runs SOCKS5 on 127.0.0.1:1819
//! - tun2socks creates a TUN adapter (`aether-tun`) and bridges TUN traffic
//!   to the SOCKS5 proxy
//! - Windows routing sends all traffic through the TUN
//!
//! All real implementation is behind `#[cfg(target_os = "windows")]`.
//! Non-Windows platforms get a no-op stub so the rest of the code compiles
//! cleanly.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Shared flag: true while the TUN manager is active. Lets other modules
/// (e.g. shutdown) check without needing a lock on the manager itself.
static TUN_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Returns true if the system tunnel is currently active.
pub fn is_active() -> bool {
    TUN_ACTIVE.load(Ordering::Relaxed)
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::net::Ipv4Addr;
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};

    /// TUN adapter IP address.
    const TUN_IP: Ipv4Addr = Ipv4Addr::new(10, 0, 0, 2);
    /// TUN adapter gateway (the "inside" end of the tunnel).
    const TUN_GW: Ipv4Addr = Ipv4Addr::new(10, 0, 0, 1);
    /// DNS to push to the TUN adapter for leaked DNS queries.
    const TUN_DNS: &str = "1.1.1.1";

    pub struct TunManager {
        tun2socks: Option<Child>,
        /// Original default gateway captured before we modified routing.
        original_gw: Option<String>,
        /// Path to the tun2socks binary (next to aether.exe in resources/binaries).
        tun2socks_path: PathBuf,
    }

    impl TunManager {
        /// Create a new manager, resolving the tun2socks path.
        /// Does not start anything — call `start()` after the SOCKS5 port is live.
        pub fn new(resource_dir: &PathBuf) -> Self {
            let tun2socks_path = resource_dir
                .join("binaries")
                .join("tun2socks.exe");
            Self {
                tun2socks: None,
                original_gw: None,
                tun2socks_path,
            }
        }

        /// Start the TUN tunnel: launch tun2socks, configure the TUN adapter,
        /// save the original route, and redirect the default gateway.
        pub fn start(&mut self, socks_addr: &str) -> Result<(), String> {
            if !self.tun2socks_path.exists() {
                return Err(format!(
                    "tun2socks.exe not found at {}",
                    self.tun2socks_path.display()
                ));
            }

            // 1. Launch tun2socks to create the TUN adapter and bridge to SOCKS5.
            log::info!(
                "Starting tun2socks: device tun://aether-tun proxy {}",
                socks_addr
            );
            let child = Command::new(&self.tun2socks_path)
                .args([
                    "-device", "tun://aether-tun",
                    "-proxy", &format!("socks5://{}", socks_addr),
                    "-loglevel", "info",
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("Failed to start tun2socks: {e}. Ensure tun2socks.exe and wintun.dll are present, and run as Administrator."))?;

            self.tun2socks = Some(child);

            // Brief pause for the TUN adapter to register with the OS.
            std::thread::sleep(std::time::Duration::from_secs(2));

            // 2. Configure the TUN adapter IP and subnet.
            run_cmd(&format!(
                "netsh interface ip set address \"aether-tun\" static {TUN_IP} 255.255.255.0 {TUN_GW}"
            ))?;

            // 3. Save the current default gateway before we change it.
            self.original_gw = capture_default_gateway();
            log::info!(
                "Original default gateway: {}",
                self.original_gw.as_deref().unwrap_or("(none)")
            );

            // 4. Set the TUN as the default gateway.
            //    First remove the existing default route, then add via TUN.
            if let Some(ref gw) = self.original_gw {
                run_cmd(&format!("route delete 0.0.0.0 mask 0.0.0.0 {gw}"))?;
            }
            run_cmd("route add 0.0.0.0 mask 0.0.0.0 10.0.0.1 metric 10")?;

            // 5. Set DNS on the TUN adapter.
            run_cmd(&format!(
                "netsh interface ip set dns \"aether-tun\" static {TUN_DNS}"
            ))?;

            TUN_ACTIVE.store(true, Ordering::Relaxed);
            log::info!("System tunnel (TUN) started successfully");
            Ok(())
        }

        /// Tear down the TUN tunnel: kill tun2socks and restore original routing.
        pub fn stop(&mut self) {
            TUN_ACTIVE.store(false, Ordering::Relaxed);

            // 1. Restore the original default gateway.
            if let Some(ref gw) = self.original_gw {
                let _ = run_cmd(&format!("route delete 0.0.0.0 mask 0.0.0.0 10.0.0.1"));
                let _ = run_cmd(&format!(
                    "route add 0.0.0.0 mask 0.0.0.0 {gw} metric 10"
                ));
                log::info!("Restored default gateway to {gw}");
            }
            self.original_gw = None;

            // 2. Kill the tun2socks process.
            if let Some(mut child) = self.tun2socks.take() {
                let _ = child.kill();
                let _ = child.wait();
                log::info!("Stopped tun2socks process");
            }

            // 3. Reset DNS on the TUN adapter (optional cleanup).
            let _ = run_cmd("netsh interface ip set dns \"aether-tun\" dhcp");
        }
    }

    impl Drop for TunManager {
        fn drop(&mut self) {
            self.stop();
        }
    }

    /// Capture the current default gateway by parsing `route print`.
    fn capture_default_gateway() -> Option<String> {
        let output = Command::new("route")
            .args(["print", "0.0.0.0"])
            .output()
            .ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        // The default route line starts with 0.0.0.0 (network) followed by
        // 0.0.0.0 (mask), then the gateway IP.
        for line in stdout.lines() {
            let trimmed = line.trim();
            // Typical line: "0.0.0.0          0.0.0.0    192.168.1.1  ..."
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
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

    /// Run a shell command and return its exit status, or an error string.
    /// Captures stderr and includes it in error messages for easier debugging.
    fn run_cmd(cmd: &str) -> Result<(), String> {
        log::debug!("TUN routing: {cmd}");
        let output = Command::new("cmd")
            .args(["/C", cmd])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("Failed to run `{cmd}`: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let msg = if stderr.trim().is_empty() {
                format!("Command failed with status {status}: {cmd}", status = output.status)
            } else {
                format!("Command failed with status {status}: {cmd}\nstderr: {stderr}", status = output.status)
            };
            log::warn!("{msg}");
            // Hint about admin privileges for routing commands
            if cmd.starts_with("netsh") || cmd.starts_with("route") {
                return Err(format!("{msg}\nNote: Run as Administrator for system tunnel commands"));
            }
            return Err(msg);
        }
        Ok(())
    }
}

// ─── Non-Windows stub ────────────────────────────────────────────────────────

#[cfg(not(target_os = "windows"))]
mod platform {
    use std::path::PathBuf;

    pub struct TunManager;

    impl TunManager {
        pub fn new(_resource_dir: &PathBuf) -> Self {
            Self
        }

        pub fn start(&mut self, _socks_addr: &str) -> Result<(), String> {
            Err("System tunnel (TUN mode) is only supported on Windows".into())
        }

        pub fn stop(&mut self) {}
    }

    impl Drop for TunManager {
        fn drop(&mut self) {
            self.stop();
        }
    }
}

// ─── Public API (re-export platform) ─────────────────────────────────────────

pub use platform::TunManager;

/// Convenience constructor.
pub fn new_manager(resource_dir: &std::path::Path) -> TunManager {
    TunManager::new(&resource_dir.to_path_buf())
}
