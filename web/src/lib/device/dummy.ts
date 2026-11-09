import { DeviceFileSchema } from "@lib";
import * as v from "valibot";
import {
  Animation,
  DeviceApi,
  DeviceConfig,
  DeviceConfigSchema,
  DeviceFile,
  DeviceMessage,
  FrameBufferListener,
  Playlist,
  WifiJoinResult,
  WifiJoinStatus,
  WifiNetworksResult,
  WifiResult,
} from "./common.ts";

export class DummyDeviceApi implements DeviceApi {
  public schema = DeviceConfigSchema;

  private networks: { ssid: string; pass: string }[] = [{ ssid: "cccc", pass: "dddd" }];
  private joinStatus: WifiJoinStatus = { attempts: 0, last_error: null, joined_ssid: "cccc", ip: "192.168.0.50", ap_fallback: false };

  public onFrameBuffer(handler: FrameBufferListener) {
    //
  }

  public async getDeviceConfig(): Promise<DeviceConfig> {
    return {
      wifi_mode: "AccessPoint",
      ap_ssid: "aaaa",
      known_wifi_networks: [...this.networks],
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
      {
        ssid: "cccc",
        signal_strength: -55,
        password_required: true,
      },
    ];
  }

  public async joinWifiNetwork(ssid: string, _pass: string): Promise<WifiJoinResult> {
    if (!this.networks.some((n) => n.ssid === ssid)) {
      this.networks.push({ ssid, pass: _pass });
    }
    this.joinStatus = { attempts: 0, last_error: null, joined_ssid: ssid, ip: "192.168.0.51", ap_fallback: false };
    return { joined: true, rebooting: false, ssid, mode: "Station" };
  }

  public async getWifiNetworks(): Promise<WifiNetworksResult> {
    return { networks: this.networks.map((n) => ({ ssid: n.ssid })), mode: "Station" };
  }

  public async forgetWifiNetwork(ssid: string): Promise<void> {
    this.networks = this.networks.filter((n) => n.ssid !== ssid);
  }

  public async getWifiJoinStatus(): Promise<WifiJoinStatus> {
    return { ...this.joinStatus };
  }

  public async getAnimationList(): Promise<Animation[]> {
    return [
      "Animation1",
      { "Animation2": 1 },
      { "Animation3": [1, "Message", true] },
      // { "Animation4": { "Prop1": 1, "Prop2": "aaa" } },
    ];
  }

  public async getPlaylist(): Promise<Playlist> {
    return [["Animation1", 1_000]];
  }

  public async sendMessage(message: DeviceMessage) {
    console.log("DummyDeviceApi.sendMessage:", message);
  }

  public async sendBinary(buffer: Uint8Array<ArrayBuffer>) {
    console.log("DummyDeviceApi.sendBinary:", buffer);
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
