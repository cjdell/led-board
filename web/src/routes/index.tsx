import { assertError, GlobalDeviceApi } from "@lib";
import { createSignal } from "solid-js";
import { createResource, Show } from "solid-js";

export function IndexRoute() {
  const [error, setError] = createSignal("");

  const api = GlobalDeviceApi;

  const [deviceConfig] = createResource(async () => {
    let retries = 5;
    while (retries-- > 0) {
      try {
        return await api.getDeviceConfig();
      } catch (err) {
        assertError(err);
        setError(err.message);
      }
    }
  });

  return (
    <div>
      <h2>Welcome to LED Board!</h2>

      <Show when={error()}>
        {(error) => <p class="text-danger">{error()}</p>}
      </Show>

      <Show when={deviceConfig()} fallback={<p>Loading…</p>}>
        {(deviceConfig) => (
          <p>
            This LED Board is called <strong>{deviceConfig().ap_ssid}</strong>
          </p>
        )}
      </Show>

      <p>
        <a href="/wifi">Set up WiFi</a> — scan for networks, connect, and manage saved networks.
      </p>
      <p>
        <a href="/remote">Remote</a> — drive the animations from here.
      </p>
    </div>
  );
}
