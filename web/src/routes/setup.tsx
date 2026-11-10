import { createResource, createSignal, For, Show } from "solid-js";
import * as v from "valibot";
import { boardStatus, type DeviceConfig, errorMessage, formatUptime, GlobalDeviceApi, livePlaylist, toast } from "@lib";
import { Body, Footer, Panel, PanelHeader, Spacer } from "../components/Panel.tsx";
import { Button } from "../components/Button.tsx";

export function SetupRoute() {
  const api = GlobalDeviceApi;

  const [deviceConfig, { mutate }] = createResource(() => api.getDeviceConfig());
  const [saving, setSaving] = createSignal(false);
  const [error, setError] = createSignal("");

  const onChange = (data: Partial<DeviceConfig>) => mutate({ ...deviceConfig()!, ...data });

  const save = async (thenReboot: boolean): Promise<boolean> => {
    setSaving(true);
    setError("");

    try {
      await api.saveDeviceConfig(v.parse(api.schema, deviceConfig()));

      if (thenReboot) await api.reboot().catch(() => {});

      toast(thenReboot ? "Saved — rebooting…" : "Settings saved ✓", "ok");

      return true;
    } catch (err) {
      setError(errorMessage(err));

      return false;
    } finally {
      setSaving(false);
    }
  };

  const [joinStatus] = createResource(async () => {
    try {
      return await api.getWifiJoinStatus();
    } catch {
      return null;
    }
  });

  const addNetwork = () => {
    const data = deviceConfig()!;

    mutate({ ...data, known_wifi_networks: [...data.known_wifi_networks, { ssid: "", pass: "" }] });
  };

  const patchNetwork = (index: number, next: Partial<{ ssid: string; pass: string }>) => {
    const data = deviceConfig()!;
    const networks = data.known_wifi_networks.map((n, i) => (i === index ? { ...n, ...next } : n));

    mutate({ ...data, known_wifi_networks: networks });
  };

  const removeNetwork = (index: number) => {
    const data = deviceConfig()!;

    mutate({ ...data, known_wifi_networks: data.known_wifi_networks.filter((_, i) => i !== index) });
  };

  return (
    <div class="page">
      <div class="split">
        {/* ── Device info ───────────────────────────────────────────────── */}
        <Panel>
          <PanelHeader>Board</PanelHeader>
          <Body>
            <div class="stat-cards">
              <Show when={boardStatus()}>
                {(s) => (
                  <>
                    <div class="stat">
                      <span class="stat__label">Uptime</span>
                      <span class="stat__value">{formatUptime(s().uptime_s)}</span>
                    </div>
                    <div class="stat">
                      <span class="stat__label">Power limit</span>
                      <span class="stat__value">{s().power_limit_pct}%</span>
                    </div>
                  </>
                )}
              </Show>
              <Show when={livePlaylist()}>
                {(pl) => (
                  <div class="stat">
                    <span class="stat__label">Playlist source</span>
                    <span class="stat__value">{pl().source === "Saved" ? "On flash" : pl().source}</span>
                  </div>
                )}
              </Show>
              <Show when={joinStatus()}>
                {(j) => (
                  <div class="stat">
                    <span class="stat__label">Address</span>
                    <span class="stat__value stat__value--mono">{j().ap_fallback ? "AP fallback" : (j().ip ?? "—")}</span>
                  </div>
                )}
              </Show>
            </div>

            <p class="hint">
              Throughput note: the browser talks to the board over the local network — the web UI, the live status push and playlist uploads
              all ride the same WebSocket.
            </p>
          </Body>
          <Footer>
            <Button
              variant="ghost"
              click={() => api.reboot().then(() => toast("Rebooting…", "ok")).catch(() => toast("Reboot sent", "ok"))}
            >
              Reboot board
            </Button>
          </Footer>
        </Panel>

        {/* ── Device config ─────────────────────────────────────────────── */}
        <Panel accent="amber">
          <PanelHeader>
            Device config
            <Spacer />
            <Show when={error()}>
              <span class="chip chip--red">fix errors</span>
            </Show>
          </PanelHeader>
          <Body>
            <Show when={deviceConfig()} fallback={<span class="skeleton">Loading…</span>}>
              {(config) => (
                <>
                  <div class="field">
                    <label>Mode</label>
                    <label class="radio-card">
                      <input
                        type="radio"
                        name="wifi_mode"
                        checked={config().wifi_mode === "Station"}
                        onChange={() => onChange({ wifi_mode: "Station" })}
                      />
                      <span>
                        <strong>Station</strong>
                        <p class="hint">Join a saved WiFi network (the strongest signal). This is normal operation.</p>
                      </span>
                    </label>
                    <label class="radio-card" style={{ "margin-top": "8px" }}>
                      <input
                        type="radio"
                        name="wifi_mode"
                        checked={config().wifi_mode === "AccessPoint"}
                        onChange={() => onChange({ wifi_mode: "AccessPoint" })}
                      />
                      <span>
                        <strong>Access Point</strong>
                        <p class="hint">Broadcast its own network so you can reach the board directly — no router needed.</p>
                      </span>
                    </label>
                  </div>

                  <div class="field">
                    <label for="ssid">Access point SSID</label>
                    <input
                      id="ssid"
                      class="input"
                      value={config().ap_ssid}
                      onInput={(e) => onChange({ ap_ssid: e.currentTarget.value })}
                    />
                    <p class="hint">Network name broadcast in Access Point mode.</p>
                  </div>

                  <div class="field">
                    <label>Saved WiFi networks (legacy config file)</label>
                    <p class="hint">
                      For day-to-day network changes use the <a href="/wifi">WiFi page</a>{" "}
                      — this list mirrors the config file for direct editing.
                    </p>
                    <Show when={config().known_wifi_networks.length > 0}>
                      <div class="list">
                        <For each={config().known_wifi_networks}>
                          {(net, i) => (
                            <div class="row">
                              <input
                                class="input grow"
                                placeholder="SSID"
                                value={net.ssid}
                                onInput={(e) => patchNetwork(i(), { ssid: e.currentTarget.value })}
                              />
                              <input
                                class="input grow"
                                placeholder="password"
                                value={net.pass}
                                onInput={(e) => patchNetwork(i(), { pass: e.currentTarget.value })}
                              />
                              <Button variant="danger" size="icon" click={() => removeNetwork(i())}>
                                ✕
                              </Button>
                            </div>
                          )}
                        </For>
                      </div>
                    </Show>
                    <Button variant="ghost" click={addNetwork}>
                      + Add network row
                    </Button>
                  </div>

                  <Show when={error()}>
                    {(e) => <div class="chip chip--red" style={{ padding: "9px 12px", "white-space": "normal" }}>{e()}</div>}
                  </Show>
                </>
              )}
            </Show>
          </Body>
          <Footer>
            <Button variant="ghost" disabled={saving()} click={() => save(false)}>
              {saving() ? "Saving…" : "Save"}
            </Button>
            <Button variant="primary" disabled={saving()} click={() => save(true)}>
              Save &amp; reboot
            </Button>
          </Footer>
        </Panel>
      </div>

      {/* ── Bench notes ───────────────────────────────────────────────── */}
      <Panel class="danger-zone">
        <PanelHeader>For the operators</PanelHeader>
        <Body>
          <p class="hint">
            Firmware updates go through <span class="mono">just ota</span>{" "}
            from the workshop machine (sha256-verified, over-cap uploads are refused). The built-in festival programme returns any time from
            the Playlists page → “Restore default”. Board logs: <span class="mono">probe-rs attach</span>{" "}
            / defmt-print, playlist state is logged by the display worker on every change.
          </p>
        </Body>
      </Panel>
    </div>
  );
}
