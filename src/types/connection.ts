// Mirrors src-tauri/src/state.rs::ConnectionState (serde adjacently-tagged
// via `#[serde(tag = "state")]`) and src-tauri/src/aether/profiles.rs.

export type ConnectionStatus =
  | { state: "Idle" }
  | { state: "Launching" }
  | { state: "Connecting" }
  | { state: "Connected"; socks_addr: string; connected_at_ms: number }
  | { state: "Reconnecting"; attempt: number; max_attempts: number }
  | { state: "Disconnecting" }
  | { state: "Error"; message: string; phase: string }

export type Protocol = "auto" | "masque" | "wireguard" | "gool"
export type ScanMode =
  "turbo" | "balanced" | "thorough" | "stealth" | "ironclad"
export type IpVersion = "v4" | "v6" | "both"
export type MasqueNoize = "firewall" | "gfw" | "light" | "off"
export type WgNoize = "balanced" | "aggressive" | "light" | "off"
export type ZeroTrustAuth = "email" | "service" | "token"

export interface ConnectionProfile {
  protocol: Protocol
  scan_mode: ScanMode
  ip_version: IpVersion
  /** Aether ≥1.1.1: reuse the last known-working gateway with a quick
   * recheck instead of a full scan. */
  quick_reconnect: boolean
  /** Aether ≥1.2.0: run MASQUE over HTTP/2 (TCP) instead of the default
   * HTTP/3 (QUIC) — for networks that block or throttle UDP. */
  masque_http2: boolean
  /** Obfuscation profile for MASQUE (firewall/gfw/off). */
  masque_noize: MasqueNoize
  /** Obfuscation profile for WireGuard/gool (balanced/aggressive/light/off). */
  wg_noize: WgNoize
  /** Local SOCKS5 listen address (--bind). Default 127.0.0.1:1819. */
  bind_address: string
  /** Aether ≥1.5.0: optional comma-separated DNS resolvers inside the tunnel. */
  dns: string
  /** Cloudflare Zero Trust team. Empty keeps the normal consumer WARP flow. */
  zero_trust_team: string
  zero_trust_auth: ZeroTrustAuth
  /** Zero Trust credentials stay in memory only; they are never persisted. */
  access_email: string
  access_client_id: string
  access_client_secret: string
  access_token: string
  /** Route HTTP/HTTPS through the organization's Gateway proxy. */
  zero_trust_gateway: boolean
  /** Aether ≥1.5.0 traffic-routing rules. */
  route_block: string
  route_direct: string
  routes_file: string
  /** Aether ≥1.6.0 extra HTTP CONNECT listener. Empty = off. */
  http_proxy: string
  /** Aether ≥1.7.0 upstream proxy URL (may embed credentials — sent via env). Empty = direct. */
  upstream: string
  /** Fixed endpoint to skip the scan. Empty = scan normally. */
  peer: string
  /** Aether ≥1.9.0 manual WARP-in-WARP hops (ip:port, port required). */
  wiw_outer: string
  wiw_inner: string
  wiw_peers: string
  /** Aether ≥1.9.0: force a fresh scan for both WIW hops. */
  wiw_scan: boolean
  /** Aether ≥1.9.0 inner MASQUE MTU (sent via env). Empty = core default. */
  masque_mtu: string
}

export interface LogLine {
  line: string
  timestamp: number
}
