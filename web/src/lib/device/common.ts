import * as v from "valibot";
import { FieldMetadata } from "../core/index.ts";

// ── Config (unchanged surface, restyled UI) ───────────────────────────────

export const DeviceConfigSchema = v.object({
  wifi_mode: v.pipe(
    v.picklist(["Station", "AccessPoint"]),
    v.description(
      `"Access Point" mode lets you reach the board directly on its own WiFi network. "Station" mode joins a saved network with the strongest signal.`,
    ),
  ),
  ap_ssid: v.pipe(
    v.string(),
    v.minLength(1),
    v.title("Access Point SSID"),
    v.description("Wireless network name to broadcast when the board is in Access Point mode"),
  ),
  known_wifi_networks: v.array(v.object({
    ssid: v.pipe(v.string(), v.minLength(1), v.title("SSID")),
    pass: v.pipe(v.string(), v.minLength(0), v.title("Password"), v.metadata(FieldMetadata({ password: true }))),
  })),
});

export type DeviceConfig = v.InferInput<typeof DeviceConfigSchema>;

export const WifiResultSchema = v.object({
  ssid: v.string(),
  signal_strength: v.number(),
  password_required: v.boolean(),
});

export type WifiResult = v.InferInput<typeof WifiResultSchema>;

export const WifiNetworkSchema = v.object({
  ssid: v.string(),
});

export type WifiNetwork = v.InferOutput<typeof WifiNetworkSchema>;

export const WifiNetworksResultSchema = v.object({
  networks: v.array(WifiNetworkSchema),
  mode: v.optional(v.string()),
});

export type WifiNetworksResult = v.InferOutput<typeof WifiNetworksResultSchema>;

export const WifiJoinResultSchema = v.object({
  joined: v.optional(v.boolean()),
  rebooting: v.optional(v.boolean()),
  ssid: v.optional(v.string()),
  mode: v.optional(v.string()),
});

export type WifiJoinResult = v.InferOutput<typeof WifiJoinResultSchema>;

export const WifiJoinStatusSchema = v.object({
  attempts: v.optional(v.number()),
  last_error: v.optional(v.nullable(v.string())),
  joined_ssid: v.optional(v.nullable(v.string())),
  ip: v.optional(v.nullable(v.string())),
  ap_fallback: v.optional(v.boolean()),
});

export type WifiJoinStatus = v.InferOutput<typeof WifiJoinStatusSchema>;

// ── Animations & playlists ────────────────────────────────────────────────

export const PrimitiveSchema = v.union([v.string(), v.number(), v.boolean()]);

export type Primitive = v.InferInput<typeof PrimitiveSchema>;

export const AnimationParamsSchema = v.union([PrimitiveSchema, v.array(PrimitiveSchema), v.record(v.string(), PrimitiveSchema)]);

export type AnimationParams = v.InferInput<typeof AnimationParamsSchema>;

/// An `AnimationEnum` on the wire: bare string for unit variants
/// ("Aurora"), `{ Kind: params }` for the rest ({"Aurora": null}).
export const AnimationSchema = v.union([
  v.string(),
  v.record(
    v.string(),
    AnimationParamsSchema,
  ),
]);

export type Animation = v.InferInput<typeof AnimationSchema>;

export const PlaylistItemSchema = v.object({
  animation: AnimationSchema,
  duration_ms: v.number(),
  label: v.optional(v.string()),
});

export type PlaylistItem = v.InferOutput<typeof PlaylistItemSchema>;

export const PlaylistSourceSchema = v.picklist(["Saved", "Default", "Live"]);

export type PlaylistSource = v.InferOutput<typeof PlaylistSourceSchema>;

export const PlaylistInfoSchema = v.object({
  playlist: v.array(PlaylistItemSchema),
  source: PlaylistSourceSchema,
  index: v.number(),
});

export type PlaylistInfo = v.InferOutput<typeof PlaylistInfoSchema>;

export const PlaylistEntrySchema = v.object({
  name: v.string(),
  size: v.number(),
});

export type PlaylistEntry = v.InferOutput<typeof PlaylistEntrySchema>;

export const PlayingSchema = v.object({
  kind: v.string(),
  title: v.string(),
  animation: AnimationSchema,
  elapsed_ms: v.number(),
  duration_ms: v.number(),
});

export type Playing = v.InferOutput<typeof PlayingSchema>;

export const BoardStatusSchema = v.object({
  now_playing: PlayingSchema,
  playlist_index: v.number(),
  playlist_len: v.number(),
  paused: v.boolean(),
  streaming: v.boolean(),
  status_screen: v.boolean(),
  power_limit_pct: v.number(),
  uptime_s: v.number(),
});

export type BoardStatus = v.InferOutput<typeof BoardStatusSchema>;

/// Accepts either the modern object item or a legacy `[animation, ms]`
/// tuple — imported files and hand-written JSON both work.
export function normalisePlaylist(raw: unknown): PlaylistItem[] {
  if (!Array.isArray(raw)) throw new Error("Playlist must be a JSON array");

  return raw.map((entry) => {
    if (Array.isArray(entry)) {
      const [animation, duration_ms] = v.parse(v.tuple([AnimationSchema, v.number()]), entry);

      return { animation, duration_ms };
    }

    const parsed = v.parse(PlaylistItemSchema, entry);

    return { ...parsed, label: parsed.label || undefined };
  });
}

/// The kind name of an `Animation` value — "Aurora", "CenteredTitle", …
export function animationKind(animation: Animation): string {
  if (typeof animation === "string") return animation;

  return Object.keys(animation)[0] ?? "?";
}

/// Short human summary of an animation for row subtitles:
/// `ScrollingMessage("Hello") → "Hello"`, `{CenteredTitle:["A","B"]} → "A / B"`.
export function animationSummary(animation: Animation): string {
  const params = typeof animation === "string" ? undefined : Object.values(animation)[0];

  if (params === undefined || params === null) return "";

  if (typeof params === "string" || typeof params === "number" || typeof params === "boolean") {
    return String(params);
  }

  if (Array.isArray(params)) {
    return params.map(String).join(" / ");
  }

  return Object.entries(params).map(([k, val]) => `${k}: ${val}`).join(", ");
}

// ── WebSocket protocol (client → board) ───────────────────────────────────

export interface AnimationMessage {
  Animation: readonly [Animation, number];
}

export interface PlaylistMessage {
  Playlist: { playlist: PlaylistItem[]; save: boolean };
}

export type DeviceMessage =
  | "Ping"
  | "Next"
  | "Reset"
  | "ResumeNow"
  | AnimationMessage
  | PlaylistMessage
  | { Pause: boolean }
  | { JumpTo: number }
  | { PowerLimit: number };

/// Server → client events (WS push frames are `{"status":…}` /
/// `{"playlist":…}`; `connected` is client-observed).
export interface DeviceEvents {
  connected?: boolean;
  status?: BoardStatus;
  playlist?: PlaylistInfo;
}

export interface DeviceApi {
  schema: typeof DeviceConfigSchema;

  /// Live-state feed: WS pushes with reconnect/backoff, plus a polling
  /// fallback while the socket is down. Returns an unsubscribe fn.
  subscribe(handler: (event: DeviceEvents) => void): () => void;

  getBoardStatus(): Promise<BoardStatus>;
  getAnimationList(): Promise<Animation[]>;

  /// The playlist the board is actually running (live, not just flash).
  getPlaylistInfo(): Promise<PlaylistInfo>;

  getDeviceConfig(): Promise<DeviceConfig>;
  saveDeviceConfig(config: DeviceConfig): Promise<void>;
  reboot(): Promise<void>;

  scanWifiNetworks(): Promise<readonly WifiResult[]>;
  joinWifiNetwork(ssid: string, pass: string): Promise<WifiJoinResult>;
  getWifiNetworks(): Promise<WifiNetworksResult>;
  forgetWifiNetwork(ssid: string): Promise<void>;
  getWifiJoinStatus(): Promise<WifiJoinStatus>;

  listPlaylists(): Promise<PlaylistEntry[]>;
  fetchPlaylist(name: string): Promise<PlaylistItem[]>;
  putPlaylist(name: string, items: PlaylistItem[]): Promise<void>;
  deletePlaylist(name: string): Promise<void>;

  send(message: DeviceMessage): Promise<void>;
  sendBinary(buffer: Uint8Array): Promise<void>;
}
