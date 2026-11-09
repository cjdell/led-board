import { RealDeviceApi } from "./real.ts";
import { DeviceApi } from "./common.ts";
import { DummyDeviceApi } from "./dummy.ts";

export * from "./real.ts";
export * from "./common.ts";
export * from "./dummy.ts";

export function getDeviceApi(): DeviceApi {
  const { hostname, port, search } = globalThis.location;

  // return new RealDeviceApi("http://led-board.local");

  // ?device=real|dummy always wins — the explicit lever for odd setups.
  const forced = new URLSearchParams(search).get("device");

  if (forced === "real") {
    return new RealDeviceApi();
  } else if (forced === "dummy") {
    return new DummyDeviceApi();
  }

  // The hosted demo has no backend at all; the vite dev server runs on a
  // pinned port (3000) without one. Everything else — the board itself and
  // the desktop simulator (localhost:8080) — serves the API from this very
  // origin, so talk to it.
  if (hostname === "demo.rustagon.chrisdell.info") {
    return new DummyDeviceApi();
  } else if ((hostname === "127.0.0.1" || hostname === "localhost") && port === "3000") {
    return new DummyDeviceApi();
  } else {
    return new RealDeviceApi();
  }
}
