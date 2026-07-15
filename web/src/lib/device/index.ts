import { RealDeviceApi } from "./real.ts";
import { DeviceApi } from "./common.ts";
import { DummyDeviceApi } from "./dummy.ts";

export * from "./real.ts";
export * from "./common.ts";
export * from "./dummy.ts";

export function getDeviceApi(): DeviceApi {
  const { hostname } = globalThis.location;

  // return new RealDeviceApi("http://led-board.local");

  if (hostname === "127.0.0.1" || hostname === "localhost" || hostname === "demo.rustagon.chrisdell.info") {
    return new DummyDeviceApi();
  } else {
    return new RealDeviceApi();
  }
}
