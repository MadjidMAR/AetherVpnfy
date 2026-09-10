import { useEffect, useRef, useState, type ReactNode } from "react"
import { ChevronDown, Info, Settings2 } from "lucide-react"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { Switch } from "@/components/ui/switch"
import { ProtocolSelect } from "@/components/ProtocolSelect"
import { ScanModeToggle } from "@/components/ScanModeToggle"
import { IpVersionToggle } from "@/components/IpVersionToggle"
import { MasqueTransportToggle } from "@/components/MasqueTransportToggle"
import { NoizeProfileToggle } from "@/components/NoizeProfileToggle"
import { BindAddressField } from "@/components/BindAddressField"
import { ProxyChainSettings } from "@/components/ProxyChainSettings"
import { ManualEndpointsSettings } from "@/components/ManualEndpointsSettings"
import { ZeroTrustSettings } from "@/components/ZeroTrustSettings"
import { RoutingSettings } from "@/components/RoutingSettings"
import { useConnectionStore } from "@/state/connectionStore"

function FieldRow({
  label,
  tooltip,
  children,
}: {
  label: string
  tooltip?: string
  children: ReactNode
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center gap-1 text-xs text-muted-foreground">
        {label}
        {tooltip && (
          <Tooltip>
            <TooltipTrigger aria-label={`About ${label}`}>
              <Info size={12} />
            </TooltipTrigger>
            <TooltipContent>{tooltip}</TooltipContent>
          </Tooltip>
        )}
      </div>
      {children}
    </div>
  )
}

/**
 * Collapsed by default — this *is* the auto-mode default: press Connect,
 * done. Everything configurable (the options Aether's own interactive setup
 * exposes — see aether/prompts.rs and profiles.rs, nothing else) plus the
 * raw log stream live behind this one disclosure.
 *
 * Deliberately animation-light: opening used to stack a Motion layout
 * spring, a 300ms tw-animate slide, an instant column reflow, and three
 * Glass filter mounts — four systems fighting read as jank. Now it's one
 * fast CSS fade/slide and nothing else.
 */
export function AdvancedPanel() {
  const logs = useConnectionStore((s) => s.logs)
  const status = useConnectionStore((s) => s.status)
  const quickReconnect = useConnectionStore((s) => s.profile.quick_reconnect)
  const setQuickReconnect = useConnectionStore((s) => s.setQuickReconnect)
  const systemTunnel = useConnectionStore((s) => s.profile.system_tunnel)
  const setSystemTunnel = useConnectionStore((s) => s.setSystemTunnel)
  const [open, setOpen] = useState(false)
  // Launch flag — locked mid-session like the other profile controls.
  const locked = status.state !== "Idle" && status.state !== "Error"
  const [autoScroll, setAutoScroll] = useState(true)
  const viewportRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (autoScroll && viewportRef.current) {
      viewportRef.current.scrollTop = viewportRef.current.scrollHeight
    }
  }, [logs, autoScroll])

  return (
    <div className="w-full max-w-sm">
      <Collapsible open={open} onOpenChange={setOpen}>
        <CollapsibleTrigger className="flex w-full items-center justify-center gap-1.5 rounded-md py-2 text-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-primary">
          <Settings2 size={14} />
          Advanced
          <ChevronDown
            size={14}
            className="transition-transform duration-150 data-[state=open]:rotate-180"
            data-state={open ? "open" : "closed"}
          />
        </CollapsibleTrigger>
        <CollapsibleContent className="overflow-hidden data-[state=closed]:animate-out data-[state=closed]:duration-100 data-[state=closed]:fade-out-0 data-[state=open]:animate-in data-[state=open]:duration-150 data-[state=open]:fade-in-0 data-[state=open]:[animation-timing-function:cubic-bezier(0.16,1,0.3,1)] data-[state=open]:slide-in-from-bottom-1">
          <div className="flex flex-col gap-4 pb-2">
            <FieldRow
              label="Protocol"
              tooltip="MASQUE disguises traffic as normal HTTPS — best against strict censorship. WireGuard is lighter and faster. gool nests two WireGuard tunnels for extra security at a speed cost."
            >
              <ProtocolSelect />
            </FieldRow>
            <FieldRow label="Scan Mode">
              <ScanModeToggle />
            </FieldRow>
            <FieldRow
              label="IP Version"
              tooltip="Which address families to search for working routes. IPv4 is the safest default on most networks."
            >
              <IpVersionToggle />
            </FieldRow>
            <FieldRow
              label="MASQUE Transport"
              tooltip="How the MASQUE tunnel carries traffic. HTTP/3 (QUIC) has the fastest handshake; HTTP/2 (TCP) looks like ordinary HTTPS and works where UDP is blocked or throttled. Only applies to the MASQUE protocol."
            >
              <MasqueTransportToggle />
            </FieldRow>
            <FieldRow
              label="Obfuscation"
              tooltip="Disguises the handshake so DPI can't fingerprint the protocol. Heavier profiles send more decoy traffic — try escalating if the default doesn't connect. Options change based on the selected protocol."
            >
              <NoizeProfileToggle />
            </FieldRow>
            <FieldRow
              label="SOCKS5 Proxy"
              tooltip="The local address Aether's SOCKS5 proxy listens on. Change the port to avoid conflicts, or enable LAN to share the tunnel with other devices on your network. v1.8+: binding anywhere but loopback is shared with the network without authentication — only do it on purpose."
            >
              <BindAddressField />
            </FieldRow>
            <FieldRow
              label="HTTP Proxy & Upstream"
              tooltip="Aether 1.6/1.7: an extra HTTP CONNECT listener for clients without SOCKS support, and chaining the whole tunnel behind another VPN or proxy app."
            >
              <ProxyChainSettings />
            </FieldRow>
            <FieldRow
              label="Manual Endpoints"
              tooltip="Aether 1.9: skip route discovery with a known-good address, pin the WARP-in-WARP hops, or tune the MASQUE inner MTU. Leave everything empty for the normal automatic scan."
            >
              <ManualEndpointsSettings />
            </FieldRow>
            <FieldRow
              label="Zero Trust (organization)"
              tooltip="Connect as a managed Cloudflare Zero Trust device instead of anonymous consumer WARP. Works with MASQUE and WireGuard. Leave the team empty for normal one-click mode."
            >
              <ZeroTrustSettings />
            </FieldRow>
            <FieldRow
              label="DNS & Routing"
              tooltip="Optional Aether 1.5 controls for DNS inside the tunnel and rules that block a destination or send it directly outside the tunnel."
            >
              <RoutingSettings />
            </FieldRow>

            <div className="flex items-center justify-between">
              <div className="flex items-center gap-1 text-xs text-muted-foreground">
                Quick reconnect
                <Tooltip>
                  <TooltipTrigger aria-label="About Quick reconnect">
                    <Info size={12} />
                  </TooltipTrigger>
                  <TooltipContent>
                    Remembers the last gateway that worked and re-tests it first
                    on the next connect, skipping the full scan when it still
                    works. Turn off to always scan fresh.
                  </TooltipContent>
                </Tooltip>
              </div>
              <Switch
                checked={quickReconnect}
                onCheckedChange={setQuickReconnect}
                disabled={locked}
                aria-label="Quick reconnect"
              />
            </div>

            <div className="flex items-center justify-between">
              <div className="flex items-center gap-1 text-xs text-muted-foreground">
                System Tunnel (Hotspot)
                <Tooltip>
                  <TooltipTrigger aria-label="About System Tunnel">
                    <Info size={12} />
                  </TooltipTrigger>
                  <TooltipContent>
                    Creates a TUN adapter and routes all system traffic through
                    the Aether tunnel. Enable this to share the VPN connection
                    via Windows hotspot. Requires tun2socks.exe and
                    Administrator privileges. Windows only.
                  </TooltipContent>
                </Tooltip>
              </div>
              <Switch
                checked={systemTunnel}
                onCheckedChange={setSystemTunnel}
                disabled={locked}
                aria-label="System Tunnel"
              />
            </div>

            <div className="flex items-center gap-2">
              <div className="h-px flex-1 bg-border" />
              <span className="text-[10px] tracking-wide text-muted-foreground uppercase">
                Logs
              </span>
              <div className="h-px flex-1 bg-border" />
            </div>

            <div
              ref={viewportRef}
              onScroll={(e) => {
                const el = e.currentTarget
                setAutoScroll(
                  el.scrollHeight - el.scrollTop - el.clientHeight < 24
                )
              }}
              className="max-h-64 overflow-y-auto rounded-md bg-black/20 p-2 font-mono text-xs text-muted-foreground ring-1 ring-white/10"
            >
              {logs.length === 0 ? (
                <p className="text-status-idle">No output yet.</p>
              ) : (
                logs.map((l, i) => <p key={i}>{l.line}</p>)
              )}
            </div>
          </div>
        </CollapsibleContent>
      </Collapsible>
    </div>
  )
}
