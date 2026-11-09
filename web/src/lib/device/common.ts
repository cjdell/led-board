import * as v from "valibot";
import { FieldMetadata } from "../core/index.ts";

export const DeviceConfigSchema = v.object({
  wifi_mode: v.pipe(
    v.picklist(["Station", "AccessPoint"]),
    v.description(
      `"Access Point" mode allows you to access your badge directly by creating its own WiFi network. "Station" mode will attempt to connect to a saved WiFi network with the strongest signal.`,
    ),
  ),
  ap_ssid: v.pipe(
    v.string(),
    v.minLength(1),
    v.title("Access Point SSID"),
    v.description("Wireless network name to broadcast when the device is in Access Point mode"),
  ),
  // ap_pass: v.pipe(v.string(), v.minLength(8), v.title("Access Point Password")),
  known_wifi_networks: v.array(v.object({
    ssid: v.pipe(v.string(), v.minLength(1), v.title("SSID")),
    pass: v.pipe(v.string(), v.minLength(0), v.title("Password"), v.metadata(FieldMetadata({ password: true }))),
  })),
});

export type DeviceConfig = v.InferInput<typeof DeviceConfigSchema>;

export const DeviceFileSchema = v.object({
  name: v.string(),
  size: v.number(),
});

export type DeviceFile = v.InferInput<typeof DeviceFileSchema>;

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

export const PrimitiveSchema = v.union([v.string(), v.number(), v.boolean()]);

export type Primitive = v.InferInput<typeof PrimitiveSchema>;

export const AnimationParamsSchema = v.union([PrimitiveSchema, v.array(PrimitiveSchema), v.record(v.string(), PrimitiveSchema)]);

export type AnimationParams = v.InferInput<typeof AnimationParamsSchema>;

export const AnimationSchema = v.union([
  v.string(),
  v.record(
    v.string(),
    AnimationParamsSchema,
  ),
]);

export type Animation = v.InferInput<typeof AnimationSchema>;

export const PlaylistSchema = v.array(v.strictTuple([AnimationSchema, v.number()]));

export type Playlist = v.InferInput<typeof PlaylistSchema>;

export interface AnimationMessage {
  Animation: readonly [Animation, number];
}

export interface PlaylistMessage {
  Playlist: { playlist: Playlist; save: boolean };
}

export type ResetMessage = "Reset";

export type DeviceMessage = AnimationMessage | PlaylistMessage | ResetMessage;

export type FrameBufferListener = (buffer: Uint8Array) => void;

export interface DeviceApi {
  onFrameBuffer: (handler: FrameBufferListener) => void;

  schema: typeof DeviceConfigSchema;

  getDeviceConfig(): Promise<DeviceConfig>;
  saveDeviceConfig(config: DeviceConfig): Promise<void>;
  reboot(): Promise<void>;

  scanWifiNetworks(): Promise<readonly WifiResult[]>;
  joinWifiNetwork(ssid: string, pass: string): Promise<WifiJoinResult>;
  getWifiNetworks(): Promise<WifiNetworksResult>;
  forgetWifiNetwork(ssid: string): Promise<void>;
  getWifiJoinStatus(): Promise<WifiJoinStatus>;

  getAnimationList(): Promise<Animation[]>;
  getPlaylist(): Promise<Playlist>;

  sendMessage(message: DeviceMessage): Promise<void>;
  sendBinary(buffer: Uint8Array): Promise<void>;
  sendFile(buffer: Uint8Array): Promise<void>;

  listFiles(): Promise<readonly DeviceFile[]>;
  readFile(filename: string): Promise<Uint8Array>;
  writeFile(filename: string, bytes: Uint8Array): Promise<void>;
  deleteFile(filename: string): Promise<void>;
}
