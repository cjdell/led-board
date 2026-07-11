import { BadgeDeviceApi } from "./badge.ts";
import { DeviceApi } from "./common.ts";
import { DummyDeviceApi } from "./dummy.ts";

export * from "./badge.ts";
export * from "./common.ts";
export * from "./dummy.ts";

export function getDeviceApi(): DeviceApi {
  const { hostname } = globalThis.location;

  if (hostname === "127.0.0.1" || hostname === "localhost" || hostname === "demo.rustagon.chrisdell.info") {
    return new DummyDeviceApi();
  } else {
    return new BadgeDeviceApi();
  }
}
