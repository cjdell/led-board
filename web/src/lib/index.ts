export * from "./core/index.ts";
export * from "./device/index.ts";

import { getDeviceApi } from "./device/index.ts";

export const CANVAS_WIDTH = 360;
export const CANVAS_HEIGHT = 400;

export const GlobalDeviceApi = getDeviceApi();
