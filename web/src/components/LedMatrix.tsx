import { createEffect, createMemo, createSignal, type JSX, onCleanup, splitProps } from "solid-js";
import { startPreview } from "@lib/preview";
import { startLiveMirror } from "@lib/liveframe";
import { GlobalDeviceApi } from "@lib";
import type { Animation } from "@lib";

interface Props extends Omit<JSX.HTMLAttributes<HTMLDivElement>, "classList"> {
  animation: Animation;
  fps?: number;
  /// Mirror the panel the board is *actually* displaying (`GET /api/frame`)
  /// instead of the local wasm render — for the "now playing" tiles. Falls
  /// back to the wasm preview while the frame feed is unavailable (dev dummy
  /// mode, old firmware, board offline).
  live?: boolean;
}

/// The panel look: a 60×40 preview canvas with the round-LED dot mask and
/// glass vignette (see .led-matrix in theme.scss). The preview re-arms
/// whenever `animation` changes.
export function LedMatrix(props: Props) {
  const [, rest] = splitProps(props, ["animation", "fps", "live"]);

  let canvas!: HTMLCanvasElement;
  let stopPreview: (() => void) | undefined;
  let stopMirror: (() => void) | undefined;

  const animation = createMemo(() => props.animation);
  const canMirror = createMemo(() => (props.live ?? false) && GlobalDeviceApi.getPanelFrame !== undefined);
  const [mirrorLive, setMirrorLive] = createSignal(false);

  createEffect(() => {
    const anim = animation();
    const mirroring = canMirror();

    if (mirroring && stopMirror === undefined) {
      stopMirror = startLiveMirror(canvas, {
        fetchFrame: () => GlobalDeviceApi.getPanelFrame?.() ?? Promise.resolve(null),
        onLive: setMirrorLive,
      });
    } else if (!mirroring && stopMirror !== undefined) {
      stopMirror();
      stopMirror = undefined;
    }

    // While the mirror is live it owns the canvas; otherwise the wasm
    // preview keeps the tile alive (and takes it back if the board disappears).
    if (mirroring && mirrorLive()) {
      stopPreview?.();
      stopPreview = undefined;
    } else {
      stopPreview?.();
      stopPreview = startPreview(canvas, anim, props.fps ?? 16);
    }
  });

  onCleanup(() => {
    stopPreview?.();
    stopMirror?.();
  });

  return (
    <div class={`led-matrix ${rest.class ?? ""}`} {...rest}>
      <canvas ref={canvas} />
    </div>
  );
}
