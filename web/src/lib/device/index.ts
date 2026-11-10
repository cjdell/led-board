import { createSignal } from "solid-js";
import { RealDeviceApi } from "./real.ts";
import { BoardStatus, DeviceApi, PlaylistInfo } from "./common.ts";
import { DummyDeviceApi } from "./dummy.ts";

export * from "./real.ts";
export * from "./common.ts";
export * from "./dummy.ts";

export function getDeviceApi(): DeviceApi {
  const { hostname, port, search } = globalThis.location;

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

export const GlobalDeviceApi = getDeviceApi();

// ── Global live feed ───────────────────────────────────────────────────────
// One subscription for the whole app: the top bar's connection dot and every
// page read these signals instead of each polling their own.

const [boardStatus, setBoardStatus] = createSignal<BoardStatus | null>(null);
const [livePlaylist, setLivePlaylist] = createSignal<PlaylistInfo | null>(null);
const [connected, setConnected] = createSignal<boolean>(false);

GlobalDeviceApi.subscribe((event) => {
  if (event.status !== undefined) setBoardStatus(event.status);
  if (event.playlist !== undefined) setLivePlaylist(event.playlist);
  if (event.connected !== undefined) setConnected(event.connected);
});

// Kick-start the playlist signal even before the socket connects.
GlobalDeviceApi.getPlaylistInfo().then(setLivePlaylist, () => {});

export { boardStatus, connected, livePlaylist };
