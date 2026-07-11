import { Button, Card } from "@components";
import { GlobalDeviceApi, WifiResult } from "@lib";
import { createResource, createSignal, For, Show } from "solid-js";
import * as v from "valibot";

export function WifiRoute() {
  const api = GlobalDeviceApi;

  const [deviceConfig, { mutate }] = createResource(() => api.getDeviceConfig());
  const [submittedCount, setSubmittedCount] = createSignal(0);
  const [scanResults, setScanResults] = createSignal<readonly WifiResult[]>();
  const [addingScanResult, setAddingScanResult] = createSignal<number | null>(null);
  const [addingScanResultPassword, setAddingScanResultPassword] = createSignal("");

  const onSaveAndReboot = async () => {
    setSubmittedCount(submittedCount() + 1);

    await api.saveDeviceConfig(v.parse(api.schema, deviceConfig()));

    await api.reboot();
  };

  const onScan = async () => {
    setScanResults(await api.scanWifiNetworks());
  };

  const onAddScanResult = (result: WifiResult, idx: number) => {
    if (addingScanResult() === idx) {
      if (addingScanResultPassword().length === 0) return;

      addNetwork(result.ssid, addingScanResultPassword());

      setAddingScanResult(null);
      setAddingScanResultPassword("");

      if (confirm("Save and reboot?")) {
        onSaveAndReboot();
      }
    }

    if (result.password_required) {
      setAddingScanResult(idx);
      return;
    }

    addNetwork(result.ssid, "");

    if (confirm("Save and reboot?")) {
      onSaveAndReboot();
    }
  };

  const addNetwork = (ssid: string, pass: string) => {
    const data = deviceConfig()!;

    mutate({ ...data, wifi_mode: "Station", known_wifi_networks: [...data.known_wifi_networks, { ssid, pass }] });
  };

  return (
    <div class="grid">
      <div class="g-col-12">
        <Card colour="info">
          <Card.Header text="Found Wifi Networks" />
          <Card.Body>
            <div class="d-flex flex-column gap-2">
              <Show when={scanResults()} fallback={`Click "Search" to search for WiFi networks`}>
                {(scanResults) => (
                  <For each={scanResults()}>
                    {(result, idx) => (
                      <div class="d-flex flex-column gap-1">
                        <div class="d-flex justify-content-between align-items-center">
                          <div class="d-flex gap-1">
                            <div class="fw-bold">[{result.signal_strength}]</div>
                            <div>{result.ssid}</div>
                            <div class="fw-bold">{result.password_required ? "[Secure]" : "[Open]"}</div>
                          </div>
                          <Button
                            colour={addingScanResult() !== idx() ? "info" : "primary"}
                            on:click={() => onAddScanResult(result, idx())}
                          >
                            {addingScanResult() !== idx() ? "Add" : "Save"}
                          </Button>
                        </div>
                        <Show when={addingScanResult() === idx()}>
                          <input
                            type="password"
                            placeholder="Password"
                            class="form-control"
                            value={addingScanResultPassword()}
                            on:change={(e) => setAddingScanResultPassword(e.target.value)}
                            on:keyup={(e) => {
                              console.log(e);
                              if (e.key === "Enter") onAddScanResult(result, idx());
                            }}
                          />
                        </Show>
                      </div>
                    )}
                  </For>
                )}
              </Show>
            </div>
          </Card.Body>
          <Card.Footer>
            <Button colour="warning" on:click={() => onScan()}>Search</Button>
          </Card.Footer>
        </Card>
      </div>
    </div>
  );
}
