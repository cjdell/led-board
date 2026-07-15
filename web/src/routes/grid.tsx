import { RouteSectionProps } from "@solidjs/router";
import { createSignal, onMount } from "solid-js";
import { GlobalDeviceApi, sleep } from "@lib";

const Width = 60;
const Height = 40;

export function GridRoute(props: RouteSectionProps) {
  const api = GlobalDeviceApi;

  const [colour, setColour] = createSignal([255, 0, 0]); // Default red
  const buffer = new Uint8Array(Width * Height * 3); // Default black

  let canvas: HTMLCanvasElement | undefined;

  // Track if we're currently drawing
  let isDrawing = false;

  // Initialize buffer to black (already is, but explicit)
  onMount(() => {
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    drawGrid(ctx, buffer, Width, Height);
  });

  const handleCanvasDown = async (e: MouseEvent | TouchEvent) => {
    e.preventDefault(); // Prevent default scrolling on touch

    await sleep(0); // Allow colour picker to change first

    isDrawing = true;
    handlePixelSet(e);
  };

  const handleCanvasMove = (e: MouseEvent | TouchEvent) => {
    if (!isDrawing) return;
    e.preventDefault();
    handlePixelSet(e);
  };

  const handleCanvasUp = () => {
    isDrawing = false;

    (async () => {
      try {
        await api.sendBinary(buffer);
      } catch (err) {
        console.error("sendBinary: Error:", err);
      }
    })();
  };

  const handleCanvasLeave = () => {
    isDrawing = false;
  };

  // Shared logic for setting pixel on mouse/touch event
  const handlePixelSet = (e: MouseEvent | TouchEvent) => {
    if (!canvas) return;

    const rect = canvas.getBoundingClientRect();
    const clientX = "touches" in e ? e.touches[0].clientX : e.clientX;
    const clientY = "touches" in e ? e.touches[0].clientY : e.clientY;

    const x = Math.floor(((clientX - rect.left) / rect.width) * Width);
    const y = Math.floor(((clientY - rect.top) / rect.height) * Height);

    // Bounds check
    if (x < 0 || x >= Width || y < 0 || y >= Height) return;

    const index = (y * Width + x) * 3;
    const [r, g, b] = colour();

    // Set pixel to current color (no toggle — always paint)
    buffer[index + 0] = r;
    buffer[index + 1] = g;
    buffer[index + 2] = b;

    // Redraw only this pixel (more efficient than full redraw)
    const ctx = canvas.getContext("2d");
    if (ctx) {
      const cellWidth = canvas.width / Width;
      const cellHeight = canvas.height / Height;
      ctx.fillStyle = `rgb(${r}, ${g}, ${b})`;
      ctx.fillRect(x * cellWidth, y * cellHeight, cellWidth, cellHeight);
      ctx.strokeStyle = "#333";
      ctx.lineWidth = 0.5;
      ctx.strokeRect(x * cellWidth, y * cellHeight, cellWidth, cellHeight);
    }
  };

  // Full grid redraw helper (used on init and for reset)
  const drawGrid = (ctx: CanvasRenderingContext2D, buf: Uint8Array, w: number, h: number) => {
    ctx.clearRect(0, 0, canvas!.width, canvas!.height);

    const cellWidth = canvas!.width / w;
    const cellHeight = canvas!.height / h;

    for (let y = 0; y < h; y++) {
      for (let x = 0; x < w; x++) {
        const index = (y * w + x) * 3;
        const r = buf[index];
        const g = buf[index + 1];
        const b = buf[index + 2];

        ctx.fillStyle = `rgb(${r}, ${g}, ${b})`;
        ctx.fillRect(x * cellWidth, y * cellHeight, cellWidth, cellHeight);

        // Draw grid lines
        ctx.strokeStyle = "#333";
        ctx.lineWidth = 0.5;
        ctx.strokeRect(x * cellWidth, y * cellHeight, cellWidth, cellHeight);
      }
    }
  };

  const currentColour = () => {
    return `#${colour()[0].toString(16).padStart(2, "0")}${colour()[1].toString(16).padStart(2, "0")}${
      colour()[2].toString(16).padStart(2, "0")
    }`;
  };

  const onChangeColour = (colour: string) => {
    if (typeof colour !== "string" || colour[0] !== "#") return;

    setColour([
      parseInt(colour.substring(1, 3), 16),
      parseInt(colour.substring(3, 5), 16),
      parseInt(colour.substring(5, 7), 16),
    ]);
  };

  console.log("currentColour", currentColour());

  return (
    <div class="grid">
      <div class="g-col-12 d-flex flex-column gap-2">
        <input style={{ height: "32px" }} type="color" value={currentColour()} on:change={(e) => onChangeColour(e.currentTarget.value)} />

        <canvas
          ref={(c) => (canvas = c)}
          width={Width * 10} // 10px per cell → 600x400 canvas
          height={Height * 10}
          style="image-rendering: pixelated; border: 1px solid #ccc; cursor: crosshair; touch-action: none;"
          onMouseDown={handleCanvasDown}
          onMouseMove={handleCanvasMove}
          onMouseUp={handleCanvasUp}
          onMouseLeave={handleCanvasLeave}
          onTouchStart={handleCanvasDown}
          onTouchMove={handleCanvasMove}
          onTouchEnd={handleCanvasUp}
        />
      </div>
    </div>
  );
}
