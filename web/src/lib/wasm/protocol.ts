export type HostIpcMessage =
  | { HexButton: HexButton }
  | { HttpResponseMeta: HttpResponseMeta }
  | { HttpResponseBody: HttpResponseBody }
  | HttpResponseComplete;

export type HexButton = "A" | "B" | "C" | "D" | "E" | "F";

export type WasmIpcMessage = { HttpRequest: HttpRequest };

export interface HttpRequest {
  url: string;
  method: string;
  headers: ([string, string])[];
}

export interface HttpResponseMeta {
  status: number;
  headers: (readonly [string, string])[];
}

export type HttpResponseBody = number[]; // Bytes

export type HttpResponseComplete = "HttpResponseComplete";
