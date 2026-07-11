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
        {(error) => <p>{error()}</p>}
      </Show>

      <Show when={deviceConfig()} fallback="Loading...">
        {(deviceConfig) => (
          <p>
            This LED Board is called <strong>{deviceConfig().ap_ssid}</strong>
          </p>
        )}
      </Show>

      <p>
        <a href="/wifi">Click here to join a WiFi network</a>
      </p>
    </div>
  );
}
