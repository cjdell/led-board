import { createEffect, createMemo, type JSX, onCleanup, splitProps } from "solid-js";
import { startPreview } from "@lib/preview";
import type { Animation } from "@lib";

interface Props extends Omit<JSX.HTMLAttributes<HTMLDivElement>, "classList"> {
  animation: Animation;
  fps?: number;
}

/// The panel look: a 60×40 preview canvas with the round-LED dot mask and
/// glass vignette (see .led-matrix in theme.scss). The preview re-arms
/// whenever `animation` changes.
export function LedMatrix(props: Props) {
  const [, rest] = splitProps(props, ["animation", "fps"]);

  let canvas!: HTMLCanvasElement;
  let stop: (() => void) | undefined;

  const animation = createMemo(() => props.animation);

  createEffect(() => {
    stop?.();
    stop = startPreview(canvas, animation(), props.fps ?? 16);
  });

  onCleanup(() => stop?.());

  return (
    <div class={`led-matrix ${rest.class ?? ""}`} {...rest}>
      <canvas ref={canvas} />
    </div>
  );
}
