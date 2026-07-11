export class HexagonCanvasManager {
  private ctx: CanvasRenderingContext2D;
  private hexagonPoints: { x: number; y: number }[] = [];
  private hexagonRadius: number = -1;
  private pointRadius: number = 16; // Radius of the circular buttons
  private isDragging: boolean = false;
  private activePoint: number | null = null;
  private screen: HTMLCanvasElement | null = null;

  // Placeholder handlers for each of the 6 hexagon points
  private pointHandler = (i: number) => console.log("Point:", i);

  constructor(private canvas: HTMLCanvasElement) {
    this.ctx = this.canvas.getContext("2d") as CanvasRenderingContext2D;

    // Set canvas size to its display size
    this.resizeCanvas();

    // Add event listeners
    this.canvas.addEventListener("click", (e) => this.handleCanvasClick(e));
    this.canvas.addEventListener("mousemove", (e) => this.handleCanvasMove(e));
    this.canvas.addEventListener("mouseleave", () => this.isDragging = false);

    // Initialize hexagon
    this.updateHexagon();
  }

  private resizeCanvas(): void {
    // Get the computed style of the canvas
    const style = self.getComputedStyle(this.canvas);
    const width = parseInt(style.width) || this.canvas.width;
    const height = parseInt(style.height) || this.canvas.height;

    // Set the canvas dimensions to match the display size
    this.canvas.width = width;
    this.canvas.height = height;

    // Update hexagon after resize
    this.updateHexagon();
  }

  private updateHexagon(): void {
    const centerX = this.canvas.width / 2;
    const centerY = this.canvas.height / 2;

    // Calculate radius to make hexagon fill the canvas
    // For a hexagon oriented with top/bottom points aligned with center,
    // the height is 2 * radius, and width is radius * sqrt(3)
    // We want to fit the hexagon within the canvas, so we use the smaller dimension
    const availableWidth = this.canvas.width;
    const availableHeight = this.canvas.height;

    // Calculate maximum radius that fits in both dimensions
    const radiusByWidth = availableWidth / Math.sqrt(3);
    const radiusByHeight = availableHeight / 2;
    this.hexagonRadius = Math.min(radiusByWidth, radiusByHeight) - 20;

    // Calculate 6 points of the hexagon (top, top-right, bottom-right, bottom, bottom-left, top-left)
    this.hexagonPoints = [];
    for (let i = 0; i < 6; i++) {
      const angle = (Math.PI * 9 / 6) + (i * Math.PI / 3); // Start at top (π/6) and go clockwise
      const x = centerX + this.hexagonRadius * Math.cos(angle);
      const y = centerY + this.hexagonRadius * Math.sin(angle);
      this.hexagonPoints.push({ x, y });
    }

    this.draw();
  }

  public drawFrameBuffer(frameBuffer: Uint8Array<ArrayBufferLike>) {
    this.screen = drawRGB565BE(frameBuffer);
    this.draw();
  }

  private draw(): void {
    this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);

    // Draw the hexagon
    this.ctx.beginPath();
    this.ctx.moveTo(this.hexagonPoints[0].x, this.hexagonPoints[0].y);

    for (let i = 1; i < this.hexagonPoints.length; i++) {
      this.ctx.lineTo(this.hexagonPoints[i].x, this.hexagonPoints[i].y);
    }

    this.ctx.closePath();
    this.ctx.fillStyle = "#33ff33"; // Blue fill color
    this.ctx.fill();
    this.ctx.strokeStyle = "#007700"; // Darker blue stroke
    this.ctx.lineWidth = 2;
    this.ctx.stroke();

    // Draw the point buttons (circles)
    for (let i = 0; i < this.hexagonPoints.length; i++) {
      const point = this.hexagonPoints[i];
      this.ctx.beginPath();

      this.ctx.arc(point.x, point.y, this.pointRadius, 0, Math.PI * 2);

      // Different color for active point
      if (this.activePoint === i) {
        this.ctx.fillStyle = "#e74c3c"; // Red for active
      } else {
        this.ctx.fillStyle = "#aaaaaa"; // Orange for normal
      }
      this.ctx.fill();

      // Add border
      this.ctx.strokeStyle = "#000000";
      this.ctx.lineWidth = 2;
      this.ctx.stroke();
    }

    // Now draw the offscreen canvas onto your main canvas
    // This will properly composite with alpha blending
    if (this.screen) {
      this.ctx.drawImage(this.screen, (this.canvas.width - WIDTH) / 2, (this.canvas.height - HEIGHT) / 2);
    }
  }

  private handleCanvasClick(e: MouseEvent): void {
    const rect = this.canvas.getBoundingClientRect();
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;

    // Check if click is within any of the point circles
    for (let i = 0; i < this.hexagonPoints.length; i++) {
      const point = this.hexagonPoints[i];
      const distance = Math.sqrt(
        Math.pow(mouseX - point.x, 2) + Math.pow(mouseY - point.y, 2),
      );

      if (distance <= this.pointRadius) {
        // Trigger the corresponding handler
        this.pointHandler(i);
        this.activePoint = i;
        this.draw();
        setTimeout(() => {
          this.activePoint = null;
          this.draw();
        }, 200); // Reset after 200ms for visual feedback
        return;
      }
    }
  }

  private handleCanvasMove(e: MouseEvent): void {
    const rect = this.canvas.getBoundingClientRect();
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;

    // Check if mouse is over any point circle
    let hoveredPoint = null;
    for (let i = 0; i < this.hexagonPoints.length; i++) {
      const point = this.hexagonPoints[i];
      const distance = Math.sqrt(
        Math.pow(mouseX - point.x, 2) + Math.pow(mouseY - point.y, 2),
      );

      if (distance <= this.pointRadius) {
        hoveredPoint = i;
        break;
      }
    }

    // Change cursor if hovering over a point
    this.canvas.style.cursor = hoveredPoint !== null ? "pointer" : "default";

    // If we're dragging, update the point position
    if (this.isDragging && this.activePoint !== null) {
      // This would allow dragging points if you wanted to implement it
      // For now, we're just using click handlers
    }
  }

  // Public method to update point handlers
  public setPointHandler(handler: (i: number) => void): void {
    this.pointHandler = handler;
  }

  // Public method to refresh the canvas
  public refresh(): void {
    this.updateHexagon();
  }

  // Public method to get canvas dimensions
  public getCanvasDimensions(): { width: number; height: number } {
    return { width: this.canvas.width, height: this.canvas.height };
  }

  // Public method to get hexagon points
  public getHexagonPoints(): { x: number; y: number }[] {
    return [...this.hexagonPoints];
  }
}

import { HEIGHT, WIDTH } from "@lib";

export function drawRGB565BE(uint8Array: Uint8Array) {
  const canvas = document.createElement("canvas");

  canvas.width = WIDTH;
  canvas.height = HEIGHT;

  const ctx = canvas.getContext("2d")!;
  const imageData = ctx.createImageData(WIDTH, HEIGHT);
  const data = imageData.data;

  const screenRadius = WIDTH / 2;

  for (let i = 0; i < uint8Array.length; i += 2) {
    const byteIndex = i * 2; // Each RGB565 pixel becomes 4 RGBA bytes
    const x = (i / 2) % WIDTH;
    const y = ((i / 2) / WIDTH) | 0;

    const low = uint8Array[i];
    const high = uint8Array[i + 1];
    const rgb565 = high | (low << 8);

    const r5 = (rgb565 >> 11) & 0x1f;
    const g6 = (rgb565 >> 5) & 0x3f;
    const b5 = rgb565 & 0x1f;

    const r = (r5 * 255 + 15) / 31;
    const g = (g6 * 255 + 31) / 63;
    const b = (b5 * 255 + 15) / 31;

    const alpha = Math.sqrt(Math.pow(x - WIDTH / 2, 2) + Math.pow(y - HEIGHT / 2, 2)) < screenRadius ? 255 : 0;

    if (alpha > 0) {
      data[byteIndex + 0] = r; // R
      data[byteIndex + 1] = g; // G
      data[byteIndex + 2] = b; // B
      data[byteIndex + 3] = alpha; // A
    }
  }

  // Put the image data on the offscreen canvas
  ctx.putImageData(imageData, 0, 0);

  return canvas;
}
