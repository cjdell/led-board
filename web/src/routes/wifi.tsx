import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { assertError, GlobalDeviceApi, toast, type WifiJoinStatus, type WifiResult } from "@lib";
import { Body, Footer, Panel, PanelHeader, Spacer } from "../components/Panel.tsx";
import { Button } from "../components/Button.tsx";
import { SignalMeter } from "../components/SignalMeter.tsx";

export function WifiRoute() {
  const api = GlobalDeviceApi;

  // Scan
  const [scanning, setScanning] = createSignal(false);
  const [scanResults, setScanResults] = createSignal<readonly WifiResult[]>([]);
  const [scanError, setScanError] = createSignal("");
  const [hasScanned, setHasScanned] = createSignal(false);

  // Connect UI
  const [expanded, setExpanded] = createSignal<string | null>(null);
  const [password, setPassword] = createSignal("");
  const [showPassword, setShowPassword] = createSignal(false);
  const [joining, setJoining] = createSignal<string | null>(null);
  const [joinError, setJoinError] = createSignal("");
  const [notice, setNotice] = createSignal("");

  // Manual entry
  const [showManual, setShowManual] = createSignal(false);
  const [manualSsid, setManualSsid] = createSignal("");
  const [manualPass, setManualPass] = createSignal("");

  // Saved networks
  const [saved, setSaved] = createSignal<string[]>([]);
  const [forgetting, setForgetting] = createSignal<string | null>(null);

  // Live board status
  const [status, setStatus] = createSignal<WifiJoinStatus | null>(null);
  const [statusUnreachable, setStatusUnreachable] = createSignal(false);

  const loadStatus = async () => {
    try {
      setStatus(await api.getWifiJoinStatus());
      setStatusUnreachable(false);
    } catch {
      setStatusUnreachable(true);
    }
  };

  const loadNetworks = async () => {
    try {
      const { networks } = await api.getWifiNetworks();

      setSaved(networks.map((n) => n.ssid));
    } catch (err) {
      assertError(err);
    }
  };

  const doScan = async () => {
    setScanning(true);
    setScanError("");

    try {
      const results = await api.scanWifiNetworks();

      setScanResults([...results].sort((a, b) => b.signal_strength - a.signal_strength));
    } catch (err) {
      assertError(err);
      setScanError(`Scan failed: ${err.message}`);
      setScanResults([]);
    }

    setHasScanned(true);
    setScanning(false);
  };

  const startConnect = (ssid: string, passwordRequired: boolean) => {
    setJoinError("");
    setNotice("");
    setPassword("");
    setShowPassword(false);

    if (passwordRequired) {
      setExpanded(expanded() === ssid ? null : ssid);
    } else {
      doJoin(ssid, "");
    }
  };

  const doJoin = async (ssid: string, pass: string) => {
    setJoining(ssid);
    setJoinError("");

    try {
      const result = await api.joinWifiNetwork(ssid, pass);

      setExpanded(null);
      setPassword("");
      setNotice(
        result.rebooting
          ? `Saved! The board is rebooting and joining "${ssid}"… this page will lose it for a few seconds.`
          : result.joined
          ? `Connected to "${ssid}".`
          : `The board is joining "${ssid}"…`,
      );
      toast(`Joining "${ssid}"…`, "ok", 2_500);
      loadNetworks();
      loadStatus();
    } catch (err) {
      assertError(err);
      setJoinError(`Could not join "${ssid}": ${err.message}`);
    }

    setJoining(null);
  };

  const doForget = async (ssid: string) => {
    setForgetting(ssid);
    setJoinError("");

    try {
      await api.forgetWifiNetwork(ssid);
      setNotice(`Forgot "${ssid}". The board stops using it from its next boot.`);
      loadNetworks();
    } catch (err) {
      assertError(err);
      setJoinError(`Could not forget "${ssid}": ${err.message}`);
    }

    setForgetting(null);
  };

  const doReboot = async () => {
    setNotice("Rebooting the board… it will be unreachable for ~10 seconds.");

    try {
      await api.reboot();
    } catch {
      // The reboot kills the connection mid-response — that is expected.
    }
  };

  onMount(() => {
    doScan();
    loadNetworks();
    loadStatus();

    const timer = setInterval(loadStatus, 4_000);

    onCleanup(() => clearInterval(timer));
  });

  const joined = () => status()?.joined_ssid;

  return (
    <div class="page">
      <div class="split">
        {/* ── Connection status ─────────────────────────────────────────── */}
        <Panel>
          <PanelHeader>Board network status</PanelHeader>
          <Body>
            <Show
              when={!statusUnreachable()}
              fallback={
                <div class="chip chip--red" style={{ padding: "10px 12px", "white-space": "normal" }}>
                  Can't reach the board — it may be rebooting, or moved to a new address. If it didn't connect, join the “LED Board” WiFi
                  network again and reopen this page.
                </div>
              }
            >
              <Show when={status()?.ap_fallback}>
                <div class="chip chip--amber" style={{ padding: "10px 12px", "white-space": "normal" }}>
                  <strong>AP fallback mode.</strong>&nbsp;The board could not join any saved network, so it is hosting “LED Board” itself.
                  Connect below to fix it.
                  <Show when={status()?.last_error}>
                    {(e) => (
                      <div class="hint" style={{ "margin-top": "6px" }}>Last error: {e()} (after {status()?.attempts ?? 0} attempts)</div>
                    )}
                  </Show>
                </div>
              </Show>
              <Show when={!status()?.ap_fallback && joined()}>
                <div class="hstack" style={{ gap: "10px" }}>
                  <span class="chip chip--green">connected</span>
                  <div class="vstack" style={{ gap: "2px" }}>
                    <strong>{joined()}</strong>
                    <Show when={status()?.ip}>
                      {(ip) => <span class="mono faint">{ip()}</span>}
                    </Show>
                  </div>
                </div>
              </Show>
              <Show when={!status()?.ap_fallback && !joined()}>
                <span class="skeleton">Connecting to a saved network…</span>
              </Show>
            </Show>
          </Body>
          <Footer>
            <Button variant="ghost" click={doReboot}>
              Reboot board
            </Button>
          </Footer>
        </Panel>

        {/* ── Saved networks ────────────────────────────────────────────── */}
        <Panel>
          <PanelHeader>
            Saved networks
            <Spacer />
            <span class="faint hint" style={{ "text-transform": "none", "letter-spacing": "0" }}>
              joins strongest signal at boot
            </span>
          </PanelHeader>
          <Body>
            <Show when={saved().length > 0} fallback={<div class="empty">No saved networks yet — pick one below.</div>}>
              <div class="list">
                <For each={saved()}>
                  {(ssid) => (
                    <div class="row">
                      <div class="row__main">
                        <span class="row__title">{ssid}</span>
                        <Show when={joined() === ssid}>
                          <span class="chip chip--green" style={{ "align-self": "flex-start", "margin-top": "4px" }}>
                            current
                          </span>
                        </Show>
                      </div>
                      <div class="row__actions">
                        <Button variant="danger" disabled={forgetting() === ssid} click={() => doForget(ssid)}>
                          {forgetting() === ssid ? "Forgetting…" : "Forget"}
                        </Button>
                      </div>
                    </div>
                  )}
                </For>
              </div>
            </Show>
          </Body>
        </Panel>
      </div>

      {/* ── Scan & connect ──────────────────────────────────────────────── */}
      <Panel accent="amber">
        <PanelHeader>
          Join a WiFi network
          <Spacer />
          <Button variant="ghost" disabled={scanning()} click={doScan}>
            {scanning() ? "Scanning…" : "⟳ Scan"}
          </Button>
        </PanelHeader>
        <Body>
          <Show when={notice()}>
            {(n) => <div class="chip chip--green" style={{ padding: "9px 12px", "white-space": "normal" }}>{n()}</div>}
          </Show>
          <Show when={joinError()}>
            {(e) => <div class="chip chip--red" style={{ padding: "9px 12px", "white-space": "normal" }}>{e()}</div>}
          </Show>
          <Show when={scanError()}>
            {(e) => <div class="chip chip--amber" style={{ padding: "9px 12px", "white-space": "normal" }}>{e()}</div>}
          </Show>

          <Show when={hasScanned() || scanning()} fallback={<span class="hint">Press “Scan” to list nearby networks.</span>}>
            <Show when={scanning()}>
              <span class="skeleton">Scanning…</span>
            </Show>
            <Show when={!scanning() && scanResults().length === 0 && hasScanned()}>
              <span class="hint">No networks found.</span>
            </Show>

            <div class="list">
              <For each={scanResults()}>
                {(net) => (
                  <div class="wifi-row wifi-row--join">
                    <SignalMeter rssi={net.signal_strength} />
                    <div class="wifi-ssid">
                      <div class="wifi-ssid__name">{net.ssid}</div>
                      <div class="wifi-ssid__sub">
                        {net.signal_strength} dBm · {net.password_required ? "🔒 secured" : "open"}
                        <Show when={saved().includes(net.ssid)}>· saved</Show>
                      </div>
                    </div>

                    <Show when={expanded() !== net.ssid}>
                      <Button variant="primary" disabled={joining() !== null} click={() => startConnect(net.ssid, net.password_required)}>
                        {joining() === net.ssid ? "Joining…" : "Connect"}
                      </Button>
                    </Show>

                    <Show when={expanded() === net.ssid}>
                      <div class="wifi-pass">
                        <input
                          class="input"
                          type={showPassword() ? "text" : "password"}
                          placeholder={`Password for "${net.ssid}"`}
                          value={password()}
                          autofocus
                          onInput={(e) => setPassword(e.currentTarget.value)}
                          onKeyUp={(e) => {
                            if (e.key === "Enter") doJoin(net.ssid, password());
                          }}
                        />
                        <label class="check nowrap">
                          <input type="checkbox" checked={showPassword()} onChange={(e) => setShowPassword(e.currentTarget.checked)} />
                          show
                        </label>
                        <Button variant="primary" disabled={joining() !== null} click={() => doJoin(net.ssid, password())}>
                          {joining() === net.ssid ? "Joining…" : "Connect"}
                        </Button>
                        <Button variant="ghost" click={() => setExpanded(null)}>
                          Cancel
                        </Button>
                      </div>
                    </Show>
                  </div>
                )}
              </For>
            </div>
          </Show>

          <Show when={showManual()}>
            <div class="vstack" style={{ "border-top": "1px solid var(--line-soft)", "padding-top": "12px" }}>
              <input
                class="input"
                placeholder="Network name (SSID)"
                value={manualSsid()}
                onInput={(e) => setManualSsid(e.currentTarget.value)}
              />
              <div class="hstack">
                <input
                  class="input grow"
                  type="password"
                  placeholder="Password (leave empty for open networks)"
                  value={manualPass()}
                  onInput={(e) => setManualPass(e.currentTarget.value)}
                  onKeyUp={(e) => {
                    if (e.key === "Enter" && manualSsid().length > 0) doJoin(manualSsid(), manualPass());
                  }}
                />
                <Button
                  variant="primary"
                  disabled={joining() !== null || manualSsid().length === 0}
                  click={() => doJoin(manualSsid(), manualPass())}
                >
                  {joining() ? "Joining…" : "Connect"}
                </Button>
              </div>
            </div>
          </Show>
        </Body>
        <Footer>
          <Button variant="ghost" click={() => setShowManual(!showManual())}>
            {showManual() ? "Hide manual entry" : "Network not listed? Add manually"}
          </Button>
        </Footer>
      </Panel>
    </div>
  );
}
