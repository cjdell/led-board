import { Button, Card } from "@components";
import { assertError, GlobalDeviceApi, WifiJoinStatus, WifiResult } from "@lib";
import { createSignal, For, Show, onCleanup, onMount } from "solid-js";

// 4-bar RSSI meter: > -60 excellent, > -70 good, > -80 fair, anything else poor.
function signalBars(rssi: number): number {
  if (rssi >= -60) return 4;
  if (rssi >= -70) return 3;
  if (rssi >= -80) return 2;
  return 1;
}

function SignalMeter(props: { rssi: number }) {
  return (
    <div class="d-flex align-items-end gap-1" title={`${props.rssi} dBm`} style={{ height: "14px" }}>
      <For each={[1, 2, 3, 4]}>
        {(bar) => (
          <div
            style={{
              width: "4px",
              "min-width": "4px",
              height: `${bar * 3 + 2}px`,
              "border-radius": "1px",
              background: bar <= signalBars(props.rssi) ? "#198754" : "rgba(128,128,128,0.35)",
            }}
          />
        )}
      </For>
    </div>
  );
}

export function WifiRoute() {
  const api = GlobalDeviceApi;

  // Scan
  const [scanning, setScanning] = createSignal(false);
  const [scanResults, setScanResults] = createSignal<readonly WifiResult[]>([]);
  const [scanError, setScanError] = createSignal("");
  const [hasScanned, setHasScanned] = createSignal(false);

  // Connect UI
  const [expanded, setExpanded] = createSignal<string | null>(null); // ssid with open password box
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
          ? `Saved! The board is rebooting and joining "${ssid}"… this page will lose the board for a few seconds.`
          : result.joined
          ? `Connected to "${ssid}".`
          : `The board is joining "${ssid}"…`,
      );
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
    const timer = setInterval(loadStatus, 4000);
    onCleanup(() => clearInterval(timer));
  });

  const connected = () => status()?.joined_ssid;

  return (
    <div class="grid">
      {/* ------------------------------------------------ Connection status */}
      <div class="g-col-12 g-col-lg-6">
        <Card colour="info">
          <Card.Header text="Board Network Status" />
          <Card.Body>
            <Show when={!statusUnreachable()} fallback={
              <div class="text-warning">
                Can't reach the board right now — it may be rebooting, or it has moved
                to a new address on your WiFi. If it did not connect, join the
                <strong> "LED Board"</strong> network again and reopen this page.
              </div>
            }>
              <Show when={status()?.ap_fallback}>
                <div class="text-warning">
                  <strong>AP fallback mode.</strong> The board could not join any saved
                  network, so it is hosting "LED Board" itself. Connect below to fix it.
                  <Show when={status()?.last_error}>
                    {(e) => <div class="small text-secondary mt-1">Last error: {e()} (after {status()?.attempts ?? 0} attempts)</div>}
                  </Show>
                </div>
              </Show>
              <Show when={!status()?.ap_fallback && connected()}>
                <div class="text-success">
                  <strong>Connected to {connected()}</strong>
                  <Show when={status()?.ip}>{(ip) => <div class="small text-secondary">IP: {ip()}</div>}</Show>
                </div>
              </Show>
              <Show when={!status()?.ap_fallback && !connected()}>
                <div class="text-secondary">Connecting to a saved network…</div>
              </Show>
            </Show>
          </Card.Body>
          <Card.Footer>
            <Button colour="secondary" on:click={doReboot}>Reboot board</Button>
          </Card.Footer>
        </Card>
      </div>

      {/* ------------------------------------------------ Saved networks */}
      <div class="g-col-12 g-col-lg-6">
        <Card colour="info">
          <Card.Header text="Saved Networks" />
          <Card.Body>
            <Show when={saved().length > 0} fallback={<div class="text-secondary">No saved networks yet — pick one below.</div>}>
              <div class="d-flex flex-column gap-2">
                <For each={saved()}>
                  {(ssid) => (
                    <div class="d-flex justify-content-between align-items-center">
                      <div class="d-flex gap-2 align-items-center">
                        <Show when={connected() === ssid}>
                          <span class="badge text-bg-success">Current</span>
                        </Show>
                        <span>{ssid}</span>
                      </div>
                      <Button
                        colour="danger"
                        disabled={forgetting() === ssid}
                        on:click={() => doForget(ssid)}
                      >
                        {forgetting() === ssid ? "Forgetting…" : "Forget"}
                      </Button>
                    </div>
                  )}
                </For>
              </div>
            </Show>
          </Card.Body>
          <Card.Footer>
            <div class="text-secondary small me-auto">The board joins the strongest saved signal at boot.</div>
          </Card.Footer>
        </Card>
      </div>

      {/* ------------------------------------------------ Scan & connect */}
      <div class="g-col-12 g-col-lg-6">
        <Card colour="primary">
          <Card.Header text="Join a WiFi Network" />
          <Card.Body>
            <Show when={notice()}>
              <div class="alert alert-success py-2 px-3">{notice()}</div>
            </Show>
            <Show when={joinError()}>
              {(e) => <div class="alert alert-danger py-2 px-3">{e()}</div>}
            </Show>

            <Show when={scanError()}>
              {(e) => <div class="alert alert-warning py-2 px-3">{e()}</div>}
            </Show>

            <Show when={hasScanned() || scanning()} fallback={
              <div class="text-secondary">Press "Scan for networks" below.</div>
            }>
              <Show when={scanning()}>
                <div class="text-secondary">Scanning…</div>
              </Show>
              <Show when={!scanning() && scanResults().length === 0 && hasScanned()}>
                <div class="text-secondary">No networks found.</div>
              </Show>
              <div class="d-flex flex-column gap-2">
                <For each={scanResults()}>
                  {(net) => (
                    <div class="d-flex flex-column gap-1">
                      <div class="d-flex justify-content-between align-items-center gap-2">
                        <div class="d-flex align-items-center gap-2">
                          <SignalMeter rssi={net.signal_strength} />
                          <span class="fw-bold">{net.ssid}</span>
                          <span class="small text-secondary">{net.password_required ? "🔒 Secured" : "Open"}</span>
                          <Show when={saved().includes(net.ssid)}>
                            <span class="badge text-bg-secondary">Saved</span>
                          </Show>
                        </div>
                        <Show when={expanded() !== net.ssid}>
                          <Button
                            colour={joining() === net.ssid ? "secondary" : "primary"}
                            disabled={joining() !== null}
                            on:click={() => startConnect(net.ssid, net.password_required)}
                          >
                            {joining() === net.ssid ? "Joining…" : "Connect"}
                          </Button>
                        </Show>
                      </div>
                      <Show when={expanded() === net.ssid}>
                        <div class="d-flex gap-2 align-items-center">
                          <input
                            type={showPassword() ? "text" : "password"}
                            class="form-control"
                            placeholder={`Password for "${net.ssid}"`}
                            value={password()}
                            autofocus
                            on:input={(e) => setPassword(e.target.value)}
                            on:keyup={(e) => {
                              if (e.key === "Enter") doJoin(net.ssid, password());
                            }}
                          />
                          <div class="form-check text-nowrap m-0">
                            <input
                              class="form-check-input"
                              type="checkbox"
                              id="showPass"
                              checked={showPassword()}
                              on:change={(e) => setShowPassword((e.target as HTMLInputElement).checked)}
                            />
                            <label class="form-check-label small" for="showPass">Show</label>
                          </div>
                          <Button
                            colour="success"
                            disabled={joining() !== null}
                            on:click={() => doJoin(net.ssid, password())}
                          >
                            {joining() === net.ssid ? "Joining…" : "Connect"}
                          </Button>
                          <Button colour="secondary" on:click={() => setExpanded(null)}>Cancel</Button>
                        </div>
                      </Show>
                    </div>
                  )}
                </For>
              </div>
            </Show>

            {/* Manual entry */}
            <Show when={showManual()}>
              <div class="d-flex flex-column gap-2 mt-3 p-3 border rounded">
                <input
                  type="text"
                  class="form-control"
                  placeholder="Network name (SSID)"
                  value={manualSsid()}
                  on:input={(e) => setManualSsid(e.target.value)}
                />
                <div class="d-flex gap-2">
                  <input
                    type="password"
                    class="form-control"
                    placeholder="Password (leave empty for open networks)"
                    value={manualPass()}
                    on:input={(e) => setManualPass(e.target.value)}
                    on:keyup={(e) => {
                      if (e.key === "Enter" && manualSsid().length > 0) doJoin(manualSsid(), manualPass());
                    }}
                  />
                  <Button
                    colour="primary"
                    disabled={joining() !== null || manualSsid().length === 0}
                    on:click={() => doJoin(manualSsid(), manualPass())}
                  >
                    {joining() ? "Joining…" : "Connect"}
                  </Button>
                </div>
              </div>
            </Show>
          </Card.Body>
          <Card.Footer>
            <Button colour="secondary" class="me-auto" on:click={() => setShowManual(!showManual())}>
              {showManual() ? "Hide manual entry" : "Network not listed? Add manually"}
            </Button>
            <Button colour="warning" disabled={scanning()} on:click={doScan}>
              {scanning() ? "Scanning…" : "Scan for networks"}
            </Button>
          </Card.Footer>
        </Card>
      </div>
    </div>
  );
}
