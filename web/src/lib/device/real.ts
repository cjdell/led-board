import * as v from "valibot";
import { sleep } from "@lib";
import {
  Animation,
  AnimationSchema,
  BoardStatus,
  BoardStatusSchema,
  DeviceApi,
  DeviceConfig,
  DeviceConfigSchema,
  DeviceEvents,
  DeviceMessage,
  PANEL_FRAME_BYTES,
  PlaylistEntry,
  PlaylistEntrySchema,
  PlaylistInfo,
  PlaylistInfoSchema,
  PlaylistItem,
  PlaylistItemSchema,
  WifiJoinResult,
  WifiJoinResultSchema,
  WifiJoinStatus,
  WifiJoinStatusSchema,
  WifiNetworksResult,
  WifiNetworksResultSchema,
  WifiResult,
  WifiResultSchema,
} from "./common.ts";

const PING_INTERVAL_MS = 10_000;
const POLL_INTERVAL_MS = 4_000;

export class RealDeviceApi implements DeviceApi {
  public schema = DeviceConfigSchema;

  private readonly baseUrl: string;
  private ws: WebSocket | null = null;
  private wsOpen = false;
  private closed = false;
  private reconnectDelay = 500;

  private readonly listeners = new Set<(event: DeviceEvents) => void>();
  private pollTimer: ReturnType<typeof setInterval> | null = null;

  constructor(host = "") {
    this.baseUrl = `${host}/api/`;

    this.connectWebSocket();
  }

  // ── live feed ────────────────────────────────────────────────────────────

  public subscribe(handler: (event: DeviceEvents) => void): () => void {
    this.listeners.add(handler);

    // Late subscribers still get the current state promptly.
    if (this.wsOpen) {
      this.getBoardStatus().then((status) => this.emit({ status }), () => {});
    }

    return () => this.listeners.delete(handler);
  }

  private emit(event: DeviceEvents) {
    for (const listener of this.listeners) {
      try {
        listener(event);
      } catch (err) {
        console.error("status listener:", err);
      }
    }
  }

  private connectWebSocket() {
    const connect = () => {
      if (this.closed) return;

      let ws: WebSocket;

      try {
        ws = new WebSocket(`${this.baseUrl}ws`, ["json"]);
      } catch {
        scheduleReconnect();
        return;
      }

      this.ws = ws;

      ws.onopen = () => {
        this.wsOpen = true;
        this.reconnectDelay = 500;
        this.stopPolling();
        this.emit({ connected: true });
      };

      ws.onmessage = (e) => {
        if (typeof e.data !== "string") return;

        let msg: { status?: unknown; playlist?: unknown };

        try {
          msg = JSON.parse(e.data);
        } catch {
          return;
        }

        if (msg.status !== undefined) {
          try {
            this.emit({ status: v.parse(BoardStatusSchema, msg.status) });
          } catch (err) {
            console.warn("bad status frame:", err);
          }
        }

        if (msg.playlist !== undefined) {
          try {
            this.emit({ playlist: v.parse(PlaylistInfoSchema, msg.playlist) });
          } catch (err) {
            console.warn("bad playlist frame:", err);
          }
        }
      };

      ws.onclose = () => {
        if (this.ws !== ws) return;

        this.ws = null;
        this.wsOpen = false;
        this.emit({ connected: false });

        scheduleReconnect();
      };

      ws.onerror = () => {
        // onclose follows on every platform; the reconnect lives there.
      };
    };

    const scheduleReconnect = () => {
      if (this.closed) return;

      const delay = this.reconnectDelay;
      this.reconnectDelay = Math.min(this.reconnectDelay * 1.8, 15_000);

      setTimeout(connect, delay);
      this.startPolling();
    };

    connect();

    // Keep-alive + activity pings for the board's idle logic.
    setInterval(() => {
      if (this.wsOpen) this.ws?.send(JSON.stringify("Ping" satisfies DeviceMessage));
    }, PING_INTERVAL_MS);
  }

  /// While the socket is down, keep the UI honest with plain GETs — a radio
  /// board behind the C3 relay drops WebSockets routinely, status shouldn't.
  private startPolling() {
    if (this.pollTimer !== null) return;

    const poll = async () => {
      if (this.wsOpen || this.closed) return this.stopPolling();

      try {
        const status = await this.getBoardStatus();
        this.emit({ status });
      } catch {
        // Board is truly unreachable — the top bar dot says it.
      }
    };

    poll();
    this.pollTimer = setInterval(poll, POLL_INTERVAL_MS);
  }

  private stopPolling() {
    if (this.pollTimer !== null) {
      clearInterval(this.pollTimer);
      this.pollTimer = null;
    }
  }

  // ── REST ─────────────────────────────────────────────────────────────────

  private async fetch(path: string, init?: RequestInit): Promise<Response> {
    const res = await fetch(`${this.baseUrl}${path}`, init);

    if (res.status !== 200) {
      throw new Error((await res.text().catch(() => "")).trim() || `HTTP ${res.status}`);
    }

    return res;
  }

  public async getBoardStatus(): Promise<BoardStatus> {
    return v.parse(BoardStatusSchema, await (await this.fetch("status")).json());
  }

  /// `GET /api/frame` — live panel mirror (raw RGB888). Null instead of
  /// throwing when the frame feed is unavailable: pre-mirror firmware
  /// answers 404, and callers degrade to the local wasm preview.
  public async getPanelFrame(): Promise<Uint8Array | null> {
    try {
      const res = await fetch(`${this.baseUrl}frame`, { cache: "no-store" });

      if (res.status !== 200) return null;

      const bytes = new Uint8Array(await res.arrayBuffer());

      return bytes.byteLength === PANEL_FRAME_BYTES ? bytes : null;
    } catch {
      return null;
    }
  }

  public async getDeviceConfig(): Promise<DeviceConfig> {
    return v.parse(DeviceConfigSchema, await (await this.fetch("config")).json());
  }

  public async saveDeviceConfig(config: DeviceConfig) {
    await this.fetch("config", {
      method: "POST",
      headers: [["Content-Type", "application/json"]],
      body: JSON.stringify(config),
    });
  }

  public async reboot() {
    await this.fetch("reboot", { method: "POST" });
  }

  public async scanWifiNetworks(): Promise<readonly WifiResult[]> {
    return v.parse(v.array(WifiResultSchema), await (await this.fetch("wifi")).json());
  }

  public async joinWifiNetwork(ssid: string, pass: string): Promise<WifiJoinResult> {
    const res = await this.fetch("wifi", {
      method: "POST",
      headers: [["Content-Type", "application/json"]],
      body: JSON.stringify({ ssid, pass }),
    });

    return v.parse(WifiJoinResultSchema, await res.json().catch(() => ({})));
  }

  public async getWifiNetworks(): Promise<WifiNetworksResult> {
    return v.parse(WifiNetworksResultSchema, await (await this.fetch("networks")).json());
  }

  public async forgetWifiNetwork(ssid: string): Promise<void> {
    await this.fetch("forget", {
      method: "POST",
      headers: [["Content-Type", "application/json"]],
      body: JSON.stringify({ ssid }),
    });
  }

  public async getWifiJoinStatus(): Promise<WifiJoinStatus> {
    return v.parse(WifiJoinStatusSchema, await (await this.fetch("join-status")).json());
  }

  public async getAnimationList(): Promise<Animation[]> {
    return v.parse(v.array(AnimationSchema), await (await this.fetch("info/animations")).json());
  }

  public async getPlaylistInfo(): Promise<PlaylistInfo> {
    return v.parse(PlaylistInfoSchema, await (await this.fetch("info/playlist")).json());
  }

  // ── playlist library ─────────────────────────────────────────────────────

  public async listPlaylists(): Promise<PlaylistEntry[]> {
    return v.parse(v.array(PlaylistEntrySchema), await (await this.fetch("playlists")).json());
  }

  public async fetchPlaylist(name: string): Promise<PlaylistItem[]> {
    const res = await this.fetch(`playlists?name=${encodeURIComponent(name)}`);

    return v.parse(v.array(PlaylistItemSchema), await res.json());
  }

  public async putPlaylist(name: string, items: PlaylistItem[]): Promise<void> {
    await this.fetch(`playlists?name=${encodeURIComponent(name)}`, {
      method: "POST",
      headers: [["Content-Type", "application/json"]],
      body: JSON.stringify(items),
    });
  }

  public async deletePlaylist(name: string): Promise<void> {
    await this.fetch(`playlists?name=${encodeURIComponent(name)}`, { method: "DELETE" });
  }

  // ── WebSocket send ───────────────────────────────────────────────────────

  private async waitForSocket(): Promise<WebSocket> {
    for (let waited = 0; waited < 20_000; waited += 250) {
      if (this.wsOpen && this.ws) return this.ws;

      await sleep(250);
    }

    throw new Error("Board not reachable — changes not applied");
  }

  public async send(message: DeviceMessage) {
    (await this.waitForSocket()).send(JSON.stringify(message));
  }

  public async sendBinary(buffer: Uint8Array<ArrayBuffer>) {
    (await this.waitForSocket()).send(buffer);
  }
}
