import { useConnectionStore } from "@/state/connectionStore"

const INPUT =
  "h-8 w-full rounded-md bg-black/20 px-2 text-xs text-foreground ring-1 ring-white/10 outline-none focus:ring-primary disabled:opacity-50"

/** Aether 1.6/1.7 proxy controls: an extra HTTP CONNECT listener for
 * non-SOCKS clients, and chaining behind another VPN/proxy app. */
export function ProxyChainSettings() {
  const profile = useConnectionStore((s) => s.profile)
  const status = useConnectionStore((s) => s.status)
  const setHttpProxy = useConnectionStore((s) => s.setHttpProxy)
  const setUpstream = useConnectionStore((s) => s.setUpstream)
  const locked = status.state !== "Idle" && status.state !== "Error"

  return (
    <div className="flex flex-col gap-2 rounded-md bg-black/10 p-2 ring-1 ring-white/10">
      <input
        type="text"
        value={profile.http_proxy}
        disabled={locked}
        onChange={(e) => setHttpProxy(e.target.value)}
        placeholder="HTTP proxy, e.g. 127.0.0.1:1820 (optional)"
        className={INPUT}
        aria-label="HTTP CONNECT proxy listen address"
      />
      <input
        type="text"
        value={profile.upstream}
        disabled={locked}
        onChange={(e) => setUpstream(e.target.value)}
        placeholder="Upstream, e.g. socks5://127.0.0.1:1080 (optional)"
        className={INPUT}
        aria-label="Upstream proxy URL"
        autoComplete="off"
        spellCheck={false}
      />
      <p className="text-[10px] leading-4 text-muted-foreground">
        HTTP serves the same tunnel for non-SOCKS clients. Upstream chains
        behind another app — <code>socks5://</code>, <code>http://</code>, or
        bare <code>host:port</code>; with an HTTP upstream, also enable
        MASQUE-over-HTTP/2 above. Credentials in the URL stay out of the process
        list.
      </p>
    </div>
  )
}
