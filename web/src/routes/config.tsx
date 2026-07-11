import { Button, Card, MagicFields } from "@components";
import { DeviceConfig, GlobalDeviceApi } from "@lib";
import { createResource, createSignal, Show } from "solid-js";
import { Suspense } from "solid-js/web";
import * as v from "valibot";

export function ConfigRoute() {
  const api = GlobalDeviceApi;

  const [deviceConfig, { mutate }] = createResource(() => api.getDeviceConfig());
  const [submittedCount, setSubmittedCount] = createSignal(0);

  const onChange = (data: Partial<DeviceConfig>) => mutate({ ...deviceConfig()!, ...data });

  const onSave = async () => {
    setSubmittedCount(submittedCount() + 1);

    await api.saveDeviceConfig(v.parse(api.schema, deviceConfig()));
  };

  const onSaveAndReboot = async () => {
    setSubmittedCount(submittedCount() + 1);

    await api.saveDeviceConfig(v.parse(api.schema, deviceConfig()));

    await api.reboot();
  };

  const onAddNetwork = () => {
    addNetwork("", "");
  };

  const addNetwork = (ssid: string, pass: string) => {
    const data = deviceConfig()!;

    mutate({ ...data, wifi_mode: "Station", known_wifi_networks: [...data.known_wifi_networks, { ssid, pass }] });
  };

  return (
    <div class="grid">
      <div class="g-col-12">
        <Card colour="warning">
          <Card.Header text="Device Config" />
          <Card.Body>
            <Suspense>
              <Show when={deviceConfig()}>
                {(deviceConfig) => (
                  <MagicFields
                    schema={api.schema}
                    data={deviceConfig()}
                    onChange={onChange}
                    validation={submittedCount() > 0}
                  />
                )}
              </Show>
            </Suspense>
          </Card.Body>
          <Card.Footer>
            <Button colour="info" on:click={() => onAddNetwork()}>Add Network</Button>
            <Button colour="primary" on:click={() => onSave()}>Save</Button>
            <Button colour="warning" on:click={() => onSaveAndReboot()}>Save and Reboot</Button>
          </Card.Footer>
        </Card>
      </div>
    </div>
  );
}
