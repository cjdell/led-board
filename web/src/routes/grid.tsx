import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { boardStatus, GlobalDeviceApi, toast } from "@lib";
import { Body, Panel, PanelHeader, Spacer } from "../components/Panel.tsx";
import { Button } from "../components/Button.tsx";

const Width = 60;
const Height = 40;

const PALETTE: [number, number, number][] = [
  [255, 255, 255],
  [255, 60, 40],
  [255, 140, 20],
  [255, 210, 60],
  [60, 255, 110],
  [40, 230, 230],
  [60, 130, 255],
  [190, 90, 255],
  [255, 90, 200],
  [0, 0, 0],
];

export function GridRoute() {
  const api = GlobalDeviceApi;
  const streaming = () => boardStatus()?.streaming ?? false;

  const [colour, setColour] = createSignal<[number, number, number]>([255, 180, 84]);
  const [eraser, setEraser] = createSignal(false);
  const [mirror, setMirror] = createSignal(true);

  const buffer = new Uint8Array(Width * Height * 3); // black

  let canvas!: HTMLCanvasElement;
  let painting = false;
  let lastSend = 0;
  let sendTimer: ReturnType<typeof setTimeout> | null = null;
  let sendFailed = false;

  onMount(() => redraw());
  onCleanup(() => {
    if (sendTimer) clearTimeout(sendTimer);
  });

  const px = (x: number, y: number, r: number, g: number, b: number) => {
    if (x < 0 || x >= Width || y < 0 || y >= Height) return;

    const i = (y * Width + x) * 3;

    buffer[i] = r;
    buffer[i + 1] = g;
    buffer[i + 2] = b;
  };

  const paintAt = (clientX: number, clientY: number) => {
    const rect = canvas.getBoundingClientRect();
    const x = Math.floor(((clientX - rect.left) / rect.width) * Width);
    const y = Math.floor(((clientY - rect.top) / rect.height) * Height);

    if (x < 0 || x >= Width || y < 0 || y >= Height) return;

    const [r, g, b] = eraser() ? [0, 0, 0] : colour();

    px(x, y, r, g, b);

    if (mirror()) px(Width - 1 - x, y, r, g, b);

    redrawPixel(x, y);
    if (mirror()) redrawPixel(Width - 1 - x, y);
  };

  const send = (throttled: boolean) => {
    if (throttled) {
      const now = Date.now();

      if (now - lastSend < 90) {
        // Coalesce — one trailing send covers the final strokes.
        if (sendTimer === null) {
          sendTimer = setTimeout(() => {
            sendTimer = null;
            send(true);
          }, 90);
        }

        return;
      }

      lastSend = now;
    }

    api
      .sendBinary(buffer)
      .then(() => {
        sendFailed = false;
      })
      .catch(() => {
        if (!sendFailed) {
          sendFailed = true;
          toast("Board offline — pixels stay on this page", "error");
        }
      });
  };

  const onDown = (e: PointerEvent) => {
    e.preventDefault();
    canvas.setPointerCapture?.(e.pointerId);
    painting = true;
    sendFailed = false;
    paintAt(e.clientX, e.clientY);
    send(true);
  };

  const onMove = (e: PointerEvent) => {
    if (!painting) return;

    paintAt(e.clientX, e.clientY);
    send(true);
  };

  const onUp = () => {
    if (!painting) return;

    painting = false;
    send(false);
  };

  const redraw = () => {
    for (let y = 0; y < Height; y++) {
      for (let x = 0; x < Width; x++) {
        redrawPixel(x, y);
      }
    }
  };

  const redrawPixel = (x: number, y: number) => {
    const ctx = canvas.getContext("2d")!;
    const cell = canvas.width / Width;
    const i = (y * Width + x) * 3;

    ctx.fillStyle = `rgb(${buffer[i]},${buffer[i + 1]},${buffer[i + 2]})`;
    ctx.fillRect(x * cell, y * cell, cell, cell);

    ctx.strokeStyle = "rgba(40,52,74,0.5)";
    ctx.lineWidth = 0.6;
    ctx.strokeRect(x * cell + 0.5, y * cell + 0.5, cell - 1, cell - 1);
  };

  const clear = () => {
    buffer.fill(0);
    redraw();
    send(false);
  };

  const currentHex = () =>
    `#${
      colour()
        .map((c) => c.toString(16).padStart(2, "0"))
        .join("")
    }`;

  return (
    <div class="page">
      <Panel accent="cyan">
        <PanelHeader>
          Pixel painter
          <Spacer />
          <Show when={streaming()}>
            <span class="chip chip--cyan">streaming to the board</span>
          </Show>
          <Button variant="ghost" click={() => api.send("ResumeNow").catch(() => {})}>
            ↩ Back to playlist
          </Button>
        </PanelHeader>
        <Body>
          <div class="painter">
            <div class="swatches">
              <div class="swatches__colours">
                <input
                  type="color"
                  value={currentHex()}
                  onInput={(e) => {
                    const hex = e.currentTarget.value;

                    setColour([parseInt(hex.slice(1, 3), 16), parseInt(hex.slice(3, 5), 16), parseInt(hex.slice(5, 7), 16)]);
                    setEraser(false);
                  }}
                />
                {PALETTE.map(([r, g, b]) => (
                  <button
                    class={!eraser() && colour()[0] === r && colour()[1] === g && colour()[2] === b ? "selected" : ""}
                    style={{ background: `rgb(${r},${g},${b})` }}
                    title={`rgb(${r},${g},${b})`}
                    onClick={() => {
                      setColour([r, g, b]);
                      setEraser(false);
                    }}
                  />
                ))}
              </div>

              <div class="swatches__tools">
                <Button on={eraser()} variant="default" click={() => setEraser(!eraser())}>
                  Eraser
                </Button>
                <Button on={mirror()} variant="default" title="Paint both halves at once" click={() => setMirror(!mirror())}>
                  Mirror
                </Button>
                <Button variant="danger" click={clear}>
                  Clear
                </Button>
              </div>
            </div>

            <canvas
              ref={canvas}
              width={Width * 10}
              height={Height * 10}
              onPointerDown={onDown}
              onPointerMove={onMove}
              onPointerUp={onUp}
              onPointerLeave={onUp}
              onPointerCancel={onUp}
            />

            <p class="hint">
              Painting takes over the panel; the playlist resumes automatically ~10&nbsp;s after you stop (or use “Back to playlist”).
            </p>
          </div>
        </Body>
      </Panel>
    </div>
  );
}
