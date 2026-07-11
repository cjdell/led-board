import { sleep, WifiResultSchema } from "@lib";
import * as v from "valibot";
import { DeviceApi, DeviceConfig, DeviceConfigSchema, DeviceFile, DeviceMessage, FrameBufferListener, WifiResult } from "./common.ts";

export class BadgeDeviceApi implements DeviceApi {
  public schema = DeviceConfigSchema;

  private readonly baseUrl;
  private ws: WebSocket | null = null;

  private frameBufferListener: FrameBufferListener | null = null;

  constructor(host = "") {
    this.baseUrl = `${host}/api/`;

    this.connectWebSocket();
  }

  private connectWebSocket() {
    const connect = () => {
      this.ws = new WebSocket(`${this.baseUrl}ws`, ["json"]);

      this.ws.addEventListener("close", connect);

      this.ws.addEventListener("message", async (e) => {
        if (this.frameBufferListener && e.data instanceof Blob) {
          const bits = new Uint8Array(await e.data.arrayBuffer());

          const pixels = new Uint16Array(bits.length * 8);

          for (let p = 0; p < pixels.length; p += 1) {
            pixels[p] = bits[(p / 8) | 0] & (1 << p % 8) ? 0xffff : 0x0000;
          }

          this.frameBufferListener(new Uint8Array(pixels.buffer));
        }
      });
    };

    connect();
  }

  public onFrameBuffer(handler: FrameBufferListener) {
    this.frameBufferListener = handler;
  }

  public async getDeviceConfig(): Promise<DeviceConfig> {
    const res = await fetch(`${this.baseUrl}config`);
    if (res.status !== 200) {
      throw new Error(await res.text());
    }
    return v.parse(DeviceConfigSchema, await res.json());
  }

  public async saveDeviceConfig(config: DeviceConfig) {
    const json = JSON.stringify(config);
    const res = await fetch(`${this.baseUrl}config`, {
      method: "POST",
      headers: [["Content-Type", "application/json"]],
      body: json,
    });
    if (res.status !== 200) {
      throw new Error(await res.text());
    }
  }

  public async reboot() {
    await fetch(`${this.baseUrl}reboot`, { method: "POST" });
  }

  public async scanWifiNetworks(): Promise<readonly WifiResult[]> {
    const res = await fetch(`${this.baseUrl}wifi`);
    if (res.status !== 200) {
      throw new Error(await res.text());
    }
    return v.parse(v.array(WifiResultSchema), await res.json());
  }

  public async sendMessage(message: DeviceMessage) {
    while (this.ws?.readyState !== WebSocket.OPEN) {
      await sleep(1000);
      console.warn("Waiting for WebSocket...", this.ws);
    }

    this.ws.send(JSON.stringify(message));
  }

  public async sendFile(bytes: Uint8Array<ArrayBuffer>) {
    await fetch(`${this.baseUrl}receive`, {
      method: "POST",
      headers: [["Content-Type", "application/octet-stream"]],
      body: bytes,
    });
  }

  public async listFiles(): Promise<readonly DeviceFile[]> {
    const res = await fetch(`${this.baseUrl}files`);
    const json = await res.text();
    return JSON.parse(json);
  }

  public async readFile(filename: string): Promise<Uint8Array> {
    const res = await fetch(`${this.baseUrl}file?file=${encodeURIComponent(filename)}`);
    return new Uint8Array(await res.arrayBuffer());
  }

  public async writeFile(filename: string, bytes: Uint8Array<ArrayBuffer>): Promise<void> {
    // We can only accept 8.3 filenames.
    let [name, ext] = filename.split(".");
    if (ext === "wasm") ext = "wsm";
    name = name.substring(0, 8);
    ext = ext.substring(0, 3);
    filename = [name, ext].join(".");

    await fetch(`${this.baseUrl}file?file=${encodeURIComponent(filename)}`, {
      method: "POST",
      headers: [["Content-Type", "application/octet-stream"]],
      body: bytes,
    });
  }

  public async deleteFile(filename: string): Promise<void> {
    await fetch(`${this.baseUrl}file?file=${encodeURIComponent(filename)}`, { method: "DELETE" });
  }
}
