import { Info } from "lucide-react"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { Switch } from "@/components/ui/switch"
import { useConnectionStore } from "@/state/connectionStore"

const INPUT =
  "h-8 w-full rounded-md bg-black/20 px-2 text-xs text-foreground ring-1 ring-white/10 outline-none focus:ring-primary disabled:opacity-50"

/** Aether 1.9 manual endpoint controls: skip the scan with a known address,
 * name the WARP-in-WARP hops yourself, or tune the MASQUE inner MTU. */
export function ManualEndpointsSettings() {
  const profile = useConnectionStore((s) => s.profile)
  const status = useConnectionStore((s) => s.status)
  const setPeer = useConnectionStore((s) => s.setPeer)
  const setWiwOuter = useConnectionStore((s) => s.setWiwOuter)
  const setWiwInner = useConnectionStore((s) => s.setWiwInner)
  const setWiwPeers = useConnectionStore((s) => s.setWiwPeers)
  const setWiwScan = useConnectionStore((s) => s.setWiwScan)
  const setMasqueMtu = useConnectionStore((s) => s.setMasqueMtu)
  const locked = status.state !== "Idle" && status.state !== "Error"

  return (
    <div className="flex flex-col gap-2 rounded-md bg-black/10 p-2 ring-1 ring-white/10">
      <input
        type="text"
        value={profile.peer}
        disabled={locked}
        onChange={(e) => setPeer(e.target.value)}
        placeholder="Fixed endpoint, e.g. 162.159.196.1:443 (optional)"
        className={INPUT}
        aria-label="Fixed endpoint address"
        spellCheck={false}
      />
      <input
        type="text"
        value={profile.wiw_outer}
        disabled={locked}
        onChange={(e) => setWiwOuter(e.target.value)}
        placeholder="WIW outer hop, e.g. 162.159.192.1:2408 (gool)"
        className={INPUT}
        aria-label="WARP-in-WARP outer hop"
        spellCheck={false}
      />
      <input
        type="text"
        value={profile.wiw_inner}
        disabled={locked}
        onChange={(e) => setWiwInner(e.target.value)}
        placeholder="WIW inner hop, e.g. 188.114.96.1:2408 (gool)"
        className={INPUT}
        aria-label="WARP-in-WARP inner hop"
        spellCheck={false}
      />
      <input
        type="text"
        value={profile.wiw_peers}
        disabled={locked}
        onChange={(e) => setWiwPeers(e.target.value)}
        placeholder="Or both hops at once, comma-separated (optional)"
        className={INPUT}
        aria-label="WARP-in-WARP both hops"
        spellCheck={false}
      />
      <input
        type="text"
        value={profile.masque_mtu}
        disabled={locked}
        onChange={(e) => setMasqueMtu(e.target.value)}
        placeholder="MASQUE inner MTU, e.g. 1280 (optional)"
        className={INPUT}
        aria-label="MASQUE inner MTU"
        inputMode="numeric"
        spellCheck={false}
      />
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-1 text-xs text-muted-foreground">
          Force fresh WIW scan
          <Tooltip>
            <TooltipTrigger aria-label="About force fresh WIW scan">
              <Info size={12} />
            </TooltipTrigger>
            <TooltipContent>
              Scan for both WARP-in-WARP hops even if an endpoint is saved —
              ignores the remembered hops.
            </TooltipContent>
          </Tooltip>
        </div>
        <Switch
          checked={profile.wiw_scan}
          onCheckedChange={setWiwScan}
          disabled={locked}
          aria-label="Force fresh WARP-in-WARP scan"
        />
      </div>
      <p className="text-[10px] leading-4 text-muted-foreground">
        Naming both hops skips the scan; naming one scans for the other. The
        port is required — which port answers differs per network.
      </p>
    </div>
  )
}
