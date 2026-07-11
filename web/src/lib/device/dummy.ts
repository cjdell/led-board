import { DeviceFileSchema } from "@lib";
import * as v from "valibot";
import { DeviceApi, DeviceConfig, DeviceConfigSchema, DeviceFile, DeviceMessage, FrameBufferListener, WifiResult } from "./common.ts";

export class DummyDeviceApi implements DeviceApi {
  public schema = DeviceConfigSchema;

  public onFrameBuffer(handler: FrameBufferListener) {
    //
  }

  public async getDeviceConfig(): Promise<DeviceConfig> {
    return {
      owner_name: "Nameless",
      app_store_url: "http://foo",
      firmware_url: "http://foo",
      wifi_mode: "AccessPoint",
      ap_ssid: "aaaa",
      known_wifi_networks: [{
        ssid: "cccc",
        pass: "dddd",
      }],
    };
  }

  public async saveDeviceConfig(config: DeviceConfig) {
    console.log("saveDeviceConfig", config);
  }

  public async reboot() {
    console.log("reboot");
  }

  public async scanWifiNetworks(): Promise<readonly WifiResult[]> {
    return [
      {
        ssid: "Fake Network 1",
        signal_strength: -90,
        password_required: true,
      },
      {
        ssid: "Fake Network 2",
        signal_strength: -80,
        password_required: true,
      },
      {
        ssid: "Fake Network 3",
        signal_strength: -70,
        password_required: false,
      },
    ];
  }

  public async sendMessage(message: DeviceMessage) {
    console.log("DummyDeviceApi.sendMessage:", message);
  }

  public async sendFile(bytes: Uint8Array<ArrayBuffer>) {
    console.log("DummyDeviceApi.sendFile:", bytes.length);
  }

  public async listFiles(): Promise<readonly DeviceFile[]> {
    const res = await fetch("/wasm/manifest.json");

    if (res.status !== 200) {
      alert("No mainfest found. Please run: \n\ndeno task generate-manifest");
      throw new Error("No manifest!");
    }

    let json;

    try {
      json = await res.json();
    } catch {
      alert("No mainfest found. Please run: \n\ndeno task generate-manifest");
      throw new Error("No manifest!");
    }

    return v.parse(v.array(DeviceFileSchema), json);

    // return [
    //   { name: "file1.txt", size: 123 },
    //   { name: "file2.txt", size: 123 },
    //   { name: "file3.txt", size: 123 },
    //   { name: "file4.txt", size: 123 },
    //   { name: "file5.txt", size: 123 },
    // ];
  }

  public async readFile(filename: string): Promise<Uint8Array> {
    const res = await fetch(`/wasm/${filename}`);
    return new Uint8Array(await res.arrayBuffer());
  }

  public async writeFile(filename: string, bytes: Uint8Array): Promise<void> {
    console.log("DummyDeviceApi.writeFile:", filename, bytes);
  }

  public async deleteFile(filename: string): Promise<void> {
    console.log("DummyDeviceApi.deleteFile:", filename);
  }
}
