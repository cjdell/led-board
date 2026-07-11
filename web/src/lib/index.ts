export * from "./core/index.ts";
export * from "./device/index.ts";
export * from "./wasm/index.ts";

import * as Comlink from "comlink";
import WasmWorker from "./wasm/index.js?worker&inline";
import { WasmRuntime } from "./wasm/index.ts";
import { getDeviceApi } from "./device/index.ts";

export const CANVAS_WIDTH = 360;
export const CANVAS_HEIGHT = 400;

export const WasmRuntimeRemote = Comlink.wrap<typeof WasmRuntime>(new WasmWorker());

export const GlobalDeviceApi = getDeviceApi();
