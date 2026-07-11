/// <reference lib="WebWorker" />
declare const window: never;
declare const document: never;
declare const localStorage: never;
declare const sessionStorage: never;
declare const navigator: never;
declare const location: never;

import { sleep } from "../core/index.ts";
import { HostIpcMessage, WasmIpcMessage } from "./protocol.ts";
import { TimerRegister } from "./timer.ts";
import { ElementOf } from "ts-essentials";
import * as Comlink from "comlink";

export type { HexButton } from "./protocol.ts";

interface WasmCtx {
  start: number;
  memory: WebAssembly.Memory;
}

export const WIDTH = 240;
export const HEIGHT = 240;

export const Buttons = ["A", "B", "C", "D", "E", "F"] as const;

export type Button = ElementOf<typeof Buttons>;

export type FrameBufferHandler = (frameBuffer: Uint8Array) => void;

export class WasmRuntime {
  private handler: FrameBufferHandler | null = null;
  private hostIpcMessages: (readonly [number, HostIpcMessage])[] = [];
  private lock = Promise.resolve();
  private running = false;
  private lastId = 0;

  public addFrameBufferHandler(handler: FrameBufferHandler) {
    this.handler = handler;
  }

  public start(buffer: ArrayBuffer) {
    // Tell the previous program to stop
    this.running = false;

    // Wait for it to actually stop...
    return this.lock = this.lock.then(async () => {
      const tr = new TextDecoder();
      const te = new TextEncoder();

      const timerRegister = new TimerRegister();

      const imports = {
        index: {
          extern_write_stdout: (ptr: number, len: number) => {
            const buf = ctx.memory.buffer.slice(ptr, ptr + len);
            const str = tr.decode(buf);

            console.log("WASM:", str);
          },
          extern_set_lcd_buffer: (ptr: number) => {
            const size = WIDTH * HEIGHT * 2;
            const frameBuffer = new Uint8Array(ctx.memory.buffer.slice(ptr, ptr + size));

            // If we generate frames at a MILLION frames a second (it does happens!) we need to slow down before we run out of RAM...
            let i = 20_000_000;
            while (i-- > 0);

            this.handler?.(frameBuffer);
          },
          extern_register_timer: (ms: number) => {
            return timerRegister.setTimer(ms);
          },
          extern_check_timer: (id: number) => {
            return timerRegister.checkTimer(id);
          },
          extern_get_millis: () => {
            return Date.now() - ctx.start;
          },
          extern_read_host_ipc_message: (id: number, ptr: number) => {
            if (this.hostIpcMessages.length === 0) {
              throw new Error("hostIpcMessages empty");
            }

            const [_id, hostMsg] = this.hostIpcMessages.shift()!;

            if (id !== _id) throw new Error(`ID mismatch: ${id} ${_id}`);

            const hostMsgJson = JSON.stringify(hostMsg);
            const hostMsgBytes = te.encode(hostMsgJson);

            const dataView = new DataView(ctx.memory.buffer);

            for (let i = 0; i < hostMsgBytes.length; i += 1) {
              dataView.setUint8(ptr + i, hostMsgBytes[i]);
            }
          },
          extern_write_wasm_ipc_message: (ptr: number, len: number) => {
            const dataView = new DataView(ctx.memory.buffer);
            const buffer = new Uint8Array(len);

            for (let i = 0; i < len; i += 1) {
              buffer[i] = dataView.getUint8(ptr + i);
            }

            const json = tr.decode(buffer);
            const wasmIpcMessage: WasmIpcMessage = JSON.parse(json);

            const id = ++this.lastId;

            void this.handleWasmIpcMessage(id, wasmIpcMessage);

            return id;
          },
        },
      };

      const obj = await WebAssembly.instantiate(buffer, imports);

      const wasm_main = obj.instance.exports.wasm_main;
      const tick = obj.instance.exports.tick;
      const memory = obj.instance.exports.memory;

      if (typeof wasm_main !== "function") {
        throw new Error("wasm_main not a function");
      }
      if (typeof tick !== "function") {
        throw new Error("tick not a function");
      }
      if (!(memory instanceof WebAssembly.Memory)) {
        throw new Error("memory is not Memory");
      }

      const ctx: WasmCtx = {
        start: Date.now(),
        memory,
      };

      wasm_main();

      this.running = true;

      while (this.running) {
        let hostMsgId = 0;
        let hostMsgLen = 0;

        if (this.hostIpcMessages.length > 0) {
          const [id, msg] = this.hostIpcMessages[0];

          hostMsgId = id;
          hostMsgLen = JSON.stringify(msg).length;
        }

        const done = tick(hostMsgId, hostMsgLen);

        if (done) {
          console.log("==== PROGRAM COMPLETE ====");
          break;
        }

        await sleep(0);
      }
    });
  }

  public sendHostIpcMessage(msg: HostIpcMessage) {
    this.hostIpcMessages.push([0, msg]);
  }

  private async handleWasmIpcMessage(id: number, wasmIpcMessage: WasmIpcMessage) {
    if ("HttpRequest" in wasmIpcMessage) {
      const request = wasmIpcMessage.HttpRequest;

      const response = await fetch(request.url, { headers: request.headers });

      this.hostIpcMessages.push([id, {
        HttpResponseMeta: {
          status: response.status,
          headers: [...response.headers.entries()],
        },
      }]);

      const body = new Uint8Array(await response.arrayBuffer());

      this.hostIpcMessages.push([id, {
        HttpResponseBody: [...body],
      }]);

      this.hostIpcMessages.push([id, "HttpResponseComplete"]);
    }
  }
}

Comlink.expose(WasmRuntime);
