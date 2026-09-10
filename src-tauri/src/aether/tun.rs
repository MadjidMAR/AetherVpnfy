//! Windows System Tunnel (TUN mode) for hotspot/tethering sharing.
//!
//! When enabled, all system traffic routes through the Aether tunnel via a
//! TUN adapter + tun2socks bridge. Architecture:
//!
//! - Aether runs SOCKS5 on 127.0.0.1:1819
//! - tun2socks creates a Wintun adapter (`aether-tun`) and bridges TUN
//!   traffic to the SOCKS5 proxy
//! - Windows routing sends all traffic through the TUN
//! - IP forwarding is enabled so hotspot (Internet Connection Sharing)
//!   clients can also reach the internet through the TUN
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

/// Returns true if the current process has Administrator privileges.
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
    use std::io::Write;
    use std::net::Ipv4Addr;
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};

    /// TUN adapter IP address (inside tunnel).
    const TUN_IP: &str = "10.0.0.2";
    /// TUN subnet mask.
    const TUN_MASK: &str = "255.255.255.0";
    /// TUN gateway (the "inside" end of the tunnel).
    const TUN_GW: &str = "10.0.0.1";
    /// DNS to push to the TUN adapter for leaked DNS queries.
    const TUN_DNS: &str = "1.1.1.1";
    /// Name of the Wintun adapter created by tun2socks.
    const TUN_ADAPTER_NAME: &str = "aether-tun";
    /// Windows Hotspot ICS subnet prefix — interfaces in this range are the
    /// hotspot and need a route back through the TUN.
    const HOTSPOT_SUBNET: &str = "192.168.137";
    /// Maximum attempts when polling for the TUN adapter to appear.
    const TUN_POLL_ATTEMPTS: u32 = 10;

    pub struct TunManager {
        tun2socks: Option<Child>,
        /// Original default gateway captured before we modified routing.
        original_gw: Option<String>,
        /// Network interface index/name of the original gateway.
        original_iface: Option<String>,
        /// Name of the hotspot interface, if detected.
        hotspot_iface: Option<String>,
        /// Path to the tun2socks binary (next to aether.exe in resources/binaries).
        tun2socks_path: PathBuf,
    }

    /// Check if the current process is running as Administrator.
    /// `net session` exits with 0 only when elevated.
    pub fn is_running_as_admin() -> bool {
        Command::new("net")
            .args(["session"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
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
                original_iface: None,
                hotspot_iface: None,
                tun2socks_path,
            }
        }

        /// Start the TUN tunnel: launch tun2socks, configure the TUN adapter,
        /// save the original route, enable IP forwarding, and redirect the
        /// default gateway through the TUN.
        pub fn start(&mut self, socks_addr: &str) -> Result<(), String> {
            // 0. Pre-flight: must be running as Administrator.
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

            // 1. Launch tun2socks with the Wintun device syntax.
            log::info!(
                "Starting tun2socks: device wintun://{} proxy {}",
                TUN_ADAPTER_NAME,
                socks_addr
            );
            let child = Command::new(&self.tun2socks_path)
                .args([
                    "-device",
                    &format!("wintun://{}", TUN_ADAPTER_NAME),
                    "-proxy",
                    &format!("socks5://{}", socks_addr),
                    "-loglevel",
                    "info",
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| {
                    format!(
                        "Failed to start tun2socks: {e}. \
                         Ensure tun2socks.exe and wintun.dll are present, \
                         and run as Administrator."
                    )
                })?;

            self.tun2socks = Some(child);

            // 2. Poll for the TUN adapter to appear (up to 10 seconds).
            log::info!("Polling for TUN adapter '{}'...", TUN_ADAPTER_NAME);
            let mut found = false;
            for attempt in 1..=TUN_POLL_ATTEMPTS {
                std::thread::sleep(std::time::Duration::from_secs(1));
                if adapter_exists(TUN_ADAPTER_NAME) {
                    log::info!(
                        "TUN adapter '{}' appeared after {}s",
                        TUN_ADAPTER_NAME,
                        attempt
                    );
                    found = true;
                    break;
                }
            }
            if !found {
                // Clean up the tun2socks process we just spawned.
                if let Some(mut child) = self.tun2socks.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                return Err(format!(
                    "TUN adapter '{}' did not appear after {}s. \
                     Ensure wintun.dll is next to tun2socks.exe and \
                     you are running as Administrator.",
                    TUN_ADAPTER_NAME,
                    TUN_POLL_ATTEMPTS
                ));
            }

            // 3. Configure the TUN adapter IP, subnet, and gateway.
            run_cmd(&format!(
                "netsh interface ip set address \"{TUN_ADAPTER_NAME}\" \
                 static {TUN_IP} {TUN_MASK} {TUN_GW}"
            ))?;

            // 4. Set DNS on the TUN adapter.
            run_cmd(&format!(
                "netsh interface ip set dns \"{TUN_ADAPTER_NAME}\" static {TUN_DNS}"
            ))?;

            // 5. Capture the original default gateway and its interface.
            self.original_gw = capture_default_gateway();
            log::info!(
                "Original default gateway: {}",
                self.original_gw.as_deref().unwrap_or("(none)")
            );
            self.original_iface = find_original_interface();
            log::info!(
                "Original gateway interface: {}",
                self.original_iface.as_deref().unwrap_or("(unknown)")
            );

            // 6. Add a static route for the original gateway via the original
            //    interface BEFORE changing the default route.  This prevents
            //    the routing loop where traffic to the real gateway would
            //    be sent back into the TUN.
            if let Some(ref gw) = self.original_gw {
                if let Some(ref iface) = self.original_iface {
                    let _ = run_cmd(&format!(
                        "route add {gw} mask 255.255.255.255 {gw} \
                         if \"{iface}\" metric 1"
                    ));
                } else {
                    // Fallback: add without explicit interface.
                    let _ = run_cmd(&format!(
                        "route add {gw} mask 255.255.255.255 {gw} metric 1"
                    ));
                }
            }

            // 7. Enable IP forwarding so hotspot / ICS clients can share
            //    the TUN connection.
            enable_ip_forwarding()?;

            // 8. Set the TUN as the default gateway.
            //    First remove the existing default route, then add via TUN.
            if let Some(ref gw) = self.original_gw {
                run_cmd(&format!(
                    "route delete 0.0.0.0 mask 0.0.0.0 {gw}"
                ))?;
            }
            run_cmd("route add 0.0.0.0 mask 0.0.0.0 10.0.0.1 metric 5")?;

            // 9. Detect hotspot interface and route its subnet through the
            //    TUN so hotspot clients' traffic is also tunneled.
            self.hotspot_iface = find_hotspot_interface();
            if let Some(ref iface) = self.hotspot_iface {
                log::info!("Hotspot interface detected: {iface}");
                // Route the hotspot subnet (192.168.137.0/24) through the TUN.
                let _ = run_cmd(&format!(
                    "route add {HOTSPOT_SUBNET}.0 mask 255.255.255.0 \
                     {TUN_GW} metric 5"
                ));
            }

            TUN_ACTIVE.store(true, Ordering::Relaxed);
            log::info!("System tunnel (TUN) started successfully");
            Ok(())
        }

        /// Tear down the TUN tunnel: restore routing, disable IP forwarding,
        /// kill tun2socks, and reset DNS on the TUN adapter.
        pub fn stop(&mut self) {
            TUN_ACTIVE.store(false, Ordering::Relaxed);

            // 1. Remove the hotspot subnet route if we added one.
            if self.hotspot_iface.is_some() {
                let _ = run_cmd(&format!(
                    "route delete {HOTSPOT_SUBNET}.0 mask 255.255.255.0 {TUN_GW}"
                ));
            }
            self.hotspot_iface = None;

            // 2. Restore the original default gateway and remove the
            //    static route for the original gateway.
            if let Some(ref gw) = self.original_gw {
                let _ = run_cmd("route delete 0.0.0.0 mask 0.0.0.0 10.0.0.1");
                let _ = run_cmd(&format!(
                    "route add 0.0.0.0 mask 0.0.0.0 {gw} metric 10"
                ));
                let _ = run_cmd(&format!(
                    "route delete {gw} mask 255.255.255.255 {gw}"
                ));
                log::info!("Restored default gateway to {gw}");
            }
            self.original_gw = None;

            // 4. Disable IP forwarding.
            disable_ip_forwarding();

            // 5. Kill the tun2socks process.
            if let Some(mut child) = self.tun2socks.take() {
                let _ = child.kill();
                let _ = child.wait();
                log::info!("Stopped tun2socks process");
            }

            // 6. Reset DNS on the TUN adapter (optional cleanup).
            let _ = run_cmd(&format!(
                "netsh interface ip set dns \"{TUN_ADAPTER_NAME}\" dhcp"
            ));

            self.original_iface = None;
        }
    }

    impl Drop for TunManager {
        fn drop(&mut self) {
            self.stop();
        }
    }

    // ─── Helpers ───────────────────────────────────────────────────────────

    /// Check whether a network adapter with the given name exists by querying
    /// `netsh interface show interface`.
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
        for line in stdout.lines() {
            if line.to_lowercase().contains(&name_lower) {
                return true;
            }
        }
        false
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

    /// Parse `route print` to find which interface the default gateway is on.
    /// Returns the interface name (e.g. "Ethernet" or "Wi-Fi").
    fn find_original_interface() -> Option<String> {
        let output = Command::new("route")
            .args(["print", "0.0.0.0"])
            .output()
            .ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let trimmed = line.trim();
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 5
                && parts[0] == "0.0.0.0"
                && parts[1] == "0.0.0.0"
                && parts[2] != "0.0.0.0"
                && parts[2].parse::<Ipv4Addr>().is_ok()
            {
                // The interface index is typically the last field.
                // We'll use netsh to resolve the index to a name.
                if let Ok(idx) = parts.last().unwrap_or(&"").parse::<u32>() {
                    return resolve_interface_name(idx);
                }
            }
        }
        None
    }

    /// Resolve a Windows interface index to its friendly name via netsh.
    fn resolve_interface_name(index: u32) -> Option<String> {
        let output = Command::new("netsh")
            .args(["interface", "ip", "show", "addresses"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut current_name: Option<String> = None;
        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("Configuration for interface") {
                // e.g. "Configuration for interface \"Ethernet\""
                if let Some(start) = trimmed.find('"') {
                    if let Some(end) = trimmed[start + 1..].find('"') {
                        current_name = Some(
                            trimmed[start + 1..start + 1 + end].to_string(),
                        );
                    }
                }
            }
            // Look for the interface index in the "Index" line.
            if let Some(ref name) = current_name {
                if trimmed.to_lowercase().contains("index")
                    && trimmed.contains(&index.to_string())
                {
                    return Some(name.clone());
                }
            }
        }
        None
    }

    /// Detect a hotspot (Internet Connection Sharing) interface by looking
    /// for an interface with an IP in the 192.168.137.x range (Windows
    /// default ICS subnet).
    fn find_hotspot_interface() -> Option<String> {
        let output = Command::new("netsh")
            .args(["interface", "ip", "show", "addresses"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut current_name: Option<String> = None;
        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("Configuration for interface") {
                if let Some(start) = trimmed.find('"') {
                    if let Some(end) = trimmed[start + 1..].find('"') {
                        current_name = Some(
                            trimmed[start + 1..start + 1 + end].to_string(),
                        );
                    }
                }
            }
            // Look for an IP address matching the hotspot subnet.
            if let Some(ref name) = current_name {
                if trimmed.to_lowercase().starts_with("ip address:")
                    || trimmed.to_lowercase().starts_with("ip address :")
                {
                    // Extract the IP from the value part.
                    let ip_part = trimmed.split(':').last().unwrap_or("").trim();
                    if let Ok(addr) = ip_part.parse::<Ipv4Addr>() {
                        let octets = addr.octets();
                        if octets[0] == 192
                            && octets[1] == 168
                            && octets[2] == 137
                        {
                            return Some(name.clone());
                        }
                    }
                }
            }
        }
        None
    }

    /// Enable IP forwarding via the IP Helper service.
    fn enable_ip_forwarding() -> Result<(), String> {
        // Ensure iphelper is running (it provides IP forwarding).
        run_cmd("sc config iphelper start= auto")?;
        let _ = run_cmd("net start iphelper");
        Ok(())
    }

    /// Disable IP forwarding (best-effort cleanup).
    fn disable_ip_forwarding() {
        let _ = run_cmd("sc config iphelper start= demand");
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
                format!(
                    "Command failed with status {status}: {cmd}",
                    status = output.status
                )
            } else {
                format!(
                    "Command failed with status {status}: {cmd}\nstderr: {stderr}",
                    status = output.status
                )
            };
            log::warn!("{msg}");
            // Hint about admin privileges for routing commands.
            if cmd.starts_with("netsh") || cmd.starts_with("route") {
                return Err(format!(
                    "{msg}\nNote: Run as Administrator for system tunnel commands"
                ));
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

    pub fn is_running_as_admin() -> bool {
        false
    }
}

// ─── Public API (re-export platform) ─────────────────────────────────────────

pub use platform::TunManager;

/// Convenience constructor.
pub fn new_manager(resource_dir: &std::path::Path) -> TunManager {
    TunManager::new(&resource_dir.to_path_buf())
}
