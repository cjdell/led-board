import {
  Animation,
  animationKind,
  BoardStatus,
  DeviceApi,
  DeviceConfig,
  DeviceConfigSchema,
  DeviceEvents,
  DeviceMessage,
  PlaylistEntry,
  PlaylistInfo,
  PlaylistItem,
  WifiJoinResult,
  WifiJoinStatus,
  WifiNetworksResult,
  WifiResult,
} from "./common.ts";

/// The simulator's built-in festival loop, mirrored so the demo page
/// (demo.rustagon.chrisdell.info / vite dev) behaves like a real board.
const FESTIVAL: PlaylistItem[] = [
  { animation: { ScrollingMessage: "Light Up Leigh with Leigh Hackspace" }, duration_ms: 12_000, label: "Banner" },
  { animation: { CenteredTitle: ["Natures", "Magic"] }, duration_ms: 5_000 },
  { animation: "FallingLeaves", duration_ms: 12_000, label: "Section 1 · Leaves" },
  { animation: "Robin", duration_ms: 10_000, label: "Section 1 · Robins" },
  { animation: "Owl", duration_ms: 10_000, label: "Section 1 · Owls" },
  { animation: "Deer", duration_ms: 12_000, label: "Section 1 · Deer" },
  { animation: "Fireflies", duration_ms: 8_000, label: "Section 1 · Fireflies" },
  { animation: { CenteredTitle: ["Awe and", "Wonder"] }, duration_ms: 5_000 },
  { animation: "Starfield", duration_ms: 10_000, label: "Section 2 · Stars" },
  { animation: "Snowfall", duration_ms: 10_000, label: "Section 2 · Snow" },
  { animation: "SnowGlobe", duration_ms: 10_000, label: "Section 2 · Snow Globe" },
  { animation: "SnowLeopard", duration_ms: 10_000, label: "Section 2 · Snow Leopard" },
  { animation: "FrostFairies", duration_ms: 10_000, label: "Section 2 · Frost Fairies" },
  { animation: "Reindeer", duration_ms: 10_000, label: "Section 2 · Reindeer" },
  { animation: "FatherChristmas", duration_ms: 12_000, label: "Section 2 · Father Christmas" },
  { animation: "Penguins", duration_ms: 10_000, label: "Section 2 · Penguins" },
  { animation: { CenteredTitle: ["Northern", "Lights"] }, duration_ms: 5_000 },
  { animation: "Aurora", duration_ms: 20_000, label: "Section 3 · Aurora Borealis" },
  { animation: "ElectricGuitar", duration_ms: 10_000, label: "Section 3 · The Buzzcocks" },
  { animation: "VinylSpin", duration_ms: 8_000, label: "Section 3 · Vinyl Records" },
  { animation: "MadameRatcliffe", duration_ms: 8_000, label: "Section 3 · Madame Ratcliffe · Hats" },
  { animation: "Parrot", duration_ms: 8_000, label: "Section 3 · Parrots" },
  { animation: { CenteredTitle: ["Heritage", "in Motion"] }, duration_ms: 5_000 },
  { animation: "CanalBoats", duration_ms: 16_000, label: "Section 4 · Canal Boats" },
  { animation: "MillBuildings", duration_ms: 10_000, label: "Section 4 · Mill Buildings" },
  { animation: "SpinningWheel", duration_ms: 10_000, label: "Section 4 · Spinning Wheel" },
  { animation: "CircusTent", duration_ms: 10_000, label: "Section 4 · Circus · Acrobat" },
  { animation: "Elephants", duration_ms: 10_000, label: "Section 4 · Colourful Elephants" },
  { animation: { CenteredTitle: ["Leigh", "Hackspace"] }, duration_ms: 6_000 },
  { animation: { ScrollingMessage: "Light Up Leigh with Leigh Hackspace" }, duration_ms: 12_000, label: "Banner" },
];

const LIBRARY: { name: string; items: PlaylistItem[] }[] = [
  {
    name: "Parade Warmup",
    items: [
      { animation: "RainbowWheel", duration_ms: 15_000 },
      { animation: "SynthWave", duration_ms: 20_000 },
    ],
  },
];

function titleOf(item: PlaylistItem): string {
  if (item.label) return item.label;

  const kind = animationKind(item.animation);

  if (typeof item.animation === "string") return kind;

  const params = Object.values(item.animation)[0];

  if (Array.isArray(params)) return `${kind}: ${params.map(String).join(" ")}`;

  return kind;
}

export class DummyDeviceApi implements DeviceApi {
  public schema = DeviceConfigSchema;

  private networks: { ssid: string; pass: string }[] = [{ ssid: "cccc", pass: "dddd" }];
  private joinStatus: WifiJoinStatus = { attempts: 0, last_error: null, joined_ssid: "cccc", ip: "192.168.0.50", ap_fallback: false };

  private playlist: PlaylistItem[] = [...FESTIVAL];
  private source: PlaylistInfo["source"] = "Default";
  private index = 0;
  private elapsed = 0;
  private paused = false;
  private powerPct = 60;
  private override: { animation: Animation; duration_ms: number; elapsed: number } | null = null;
  private startedAt = Date.now();

  private listeners = new Set<(e: DeviceEvents) => void>();

  constructor() {
    setInterval(() => this.tick(), 1_000);
    setTimeout(() => this.emit({ connected: true }), 300);
  }

  private tick() {
    if (this.override) {
      this.override.elapsed += 1_000;

      if (this.override.elapsed > this.override.duration_ms) this.override = null;
    } else if (!this.paused && this.playlist.length > 0) {
      this.elapsed += 1_000;

      if (this.elapsed > this.playlist[this.index].duration_ms) {
        this.index = (this.index + 1) % this.playlist.length;
        this.elapsed = 0;
      }
    }

    this.emit({ status: this.status(), playlist: this.playlistInfo() });
  }

  private status(): BoardStatus {
    const item = this.playlist[Math.min(this.index, this.playlist.length - 1)];

    const playing = this.override
      ? {
        kind: animationKind(this.override.animation),
        title: animationKind(this.override.animation),
        animation: this.override.animation,
        elapsed_ms: this.override.elapsed,
        duration_ms: this.override.duration_ms,
      }
      : item
      ? {
        kind: animationKind(item.animation),
        title: titleOf(item),
        animation: item.animation,
        elapsed_ms: this.elapsed,
        duration_ms: item.duration_ms,
      }
      : { kind: "Blank", title: "—", animation: "Blank" as Animation, elapsed_ms: 0, duration_ms: 0 };

    return {
      now_playing: playing,
      playlist_index: this.index,
      playlist_len: this.playlist.length,
      paused: this.paused,
      streaming: false,
      status_screen: false,
      power_limit_pct: this.powerPct,
      uptime_s: Math.floor((Date.now() - this.startedAt) / 1000),
      firmware_sha: "dev00000",
      firmware_dirty: true,
    };
  }

  private playlistInfo(): PlaylistInfo {
    return { playlist: [...this.playlist], source: this.source, index: this.index };
  }

  private emit(event: DeviceEvents) {
    for (const listener of this.listeners) {
      try {
        listener(event);
      } catch (err) {
        console.error("listener:", err);
      }
    }
  }

  public subscribe(handler: (e: DeviceEvents) => void): () => void {
    this.listeners.add(handler);

    setTimeout(() => handler({ status: this.status(), playlist: this.playlistInfo() }), 0);

    return () => this.listeners.delete(handler);
  }

  public async getBoardStatus() {
    return this.status();
  }

  public async getDeviceConfig(): Promise<DeviceConfig> {
    return {
      wifi_mode: "AccessPoint",
      ap_ssid: "LED Board (demo)",
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
      { ssid: "Fake Network 1", signal_strength: -90, password_required: true },
      { ssid: "Fake Network 2", signal_strength: -80, password_required: true },
      { ssid: "Fake Network 3", signal_strength: -70, password_required: false },
      { ssid: "cccc", signal_strength: -55, password_required: true },
    ];
  }

  public async joinWifiNetwork(ssid: string, pass: string): Promise<WifiJoinResult> {
    if (!this.networks.some((n) => n.ssid === ssid)) {
      this.networks.push({ ssid, pass });
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
      "Blank",
      "Aurora",
      "Starfield",
      "Snowfall",
      "FallingLeaves",
      "Fireflies",
      "CanalBoats",
      "VinylSpin",
      { TwoLineMessage: ["Line 1", "Line 2"] },
      { ScrollingMessage: "Message" },
      { CenteredTitle: ["Title", "Line"] },
      { StatusCard: ["Alert", "Line"] },
      "Circles",
      "Fireworks",
      "SynthWave",
      "Rainbows",
      "TestPattern",
      "RainbowWheel",
      "SpinningCube",
      "Ship",
      "Columns",
      "Intensity",
      "BarChart",
      "LineChart",
      "EchoOfTheSoul",
      "HarmonicSpiral",
      "OctopusOfSound",
      "MagneticField",
      "SilentDancer",
    ];
  }

  public async getPlaylistInfo(): Promise<PlaylistInfo> {
    return this.playlistInfo();
  }

  public async listPlaylists(): Promise<PlaylistEntry[]> {
    return LIBRARY.map((p) => ({ name: p.name, size: JSON.stringify(p.items).length }));
  }

  public async fetchPlaylist(name: string): Promise<PlaylistItem[]> {
    const found = LIBRARY.find((p) => p.name === name);

    if (!found) throw new Error(`No playlist named ${name}`);

    return [...found.items];
  }

  public async putPlaylist(name: string, items: PlaylistItem[]): Promise<void> {
    const existing = LIBRARY.find((p) => p.name === name);

    if (existing) existing.items = [...items];
    else LIBRARY.push({ name, items: [...items] });
  }

  public async deletePlaylist(name: string): Promise<void> {
    const idx = LIBRARY.findIndex((p) => p.name === name);

    if (idx < 0) throw new Error(`No playlist named ${name}`);

    LIBRARY.splice(idx, 1);
  }

  public async send(message: DeviceMessage) {
    console.log("DummyDeviceApi.send:", message);

    if (message === "Next") {
      this.index = (this.index + 1) % Math.max(this.playlist.length, 1);
      this.elapsed = 0;
    } else if (message === "Reset") {
      this.playlist = [...FESTIVAL];
      this.source = "Default";
      this.index = 0;
      this.elapsed = 0;
    } else if (message === "ResumeNow") {
      this.paused = false;
    } else if (typeof message === "object" && "Pause" in message) {
      this.paused = message.Pause;
    } else if (typeof message === "object" && "JumpTo" in message) {
      if (message.JumpTo < this.playlist.length) {
        this.index = message.JumpTo;
        this.elapsed = 0;
      }
    } else if (typeof message === "object" && "Animation" in message) {
      const [animation, duration_ms] = message.Animation;
      this.override = { animation, duration_ms, elapsed: 0 };
    } else if (typeof message === "object" && "Playlist" in message) {
      this.playlist = message.Playlist.playlist.map((i) => ({ ...i }));
      this.source = message.Playlist.save ? "Saved" : "Live";
      this.index = 0;
      this.elapsed = 0;
    } else if (typeof message === "object" && "PowerLimit" in message) {
      this.powerPct = Math.round(message.PowerLimit * 100);
    }

    this.emit({ status: this.status(), playlist: this.playlistInfo() });
  }

  public async sendBinary(buffer: Uint8Array<ArrayBuffer>) {
    console.log("DummyDeviceApi.sendBinary:", buffer.byteLength, "bytes");
  }
}
