import { type JSX, Show } from "solid-js";
import { guardAsync } from "@lib";

type Variant = "default" | "primary" | "cyan" | "danger" | "ghost";

interface Props extends Omit<JSX.ButtonHTMLAttributes<HTMLButtonElement>, "onClick"> {
  variant?: Variant;
  size?: "sm" | "lg" | "icon";
  /// Toggle-style on state (renders the lit `.btn--on` treatment).
  on?: boolean;
  /// Spinner + disabled while true (for async work wired through `click`).
  loading?: boolean;
  click: (e: MouseEvent) => unknown;
}

/// The button primitive: `.btn` + variant classes. Async handlers route
/// thrown errors to the toast stack automatically.
export function Button(props: Props) {
  const onClick = guardAsync(async (e: MouseEvent) => {
    await props.click(e);
  });

  const classes = () => {
    const out = ["btn", `btn--${props.variant ?? "default"}`];

    if (props.size === "lg") out.push("btn--lg");
    if (props.size === "icon") out.push("btn--icon");
    if (props.on) out.push("btn--on");

    return out.join(" ");
  };

  const { variant: _v, size: _s, on: _on, loading: _l, click: _c, class: cls, ...rest } = props;

  return (
    <button
      {...rest}
      class={`${classes()} ${cls ?? ""}`}
      disabled={props.disabled || props.loading}
      on:click={onClick}
    >
      {props.children}
      <Show when={props.loading}>
        <svg class="spin" viewBox="0 0 24 24" width="14" height="14" aria-hidden>
          <circle
            cx="12"
            cy="12"
            r="9"
            fill="none"
            stroke="currentColor"
            stroke-width="3"
            stroke-dasharray="42 14"
            stroke-linecap="round"
          />
        </svg>
      </Show>
    </button>
  );
}
