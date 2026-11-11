import { For, Show } from "solid-js";
import { A } from "@solidjs/router";
import { boardStatus, connected, type RouteDefinition } from "@lib";

interface Props {
  routes: readonly RouteDefinition[];
  pathname: string;
}

export function TopBar(props: Props) {
  const isSelected = (path: string) => path === "/" ? props.pathname === "/" : props.pathname.startsWith(path);

  // Firmware identity from the live status feed (TopBar is global, so this
  // is the one place the flashed commit is always readable).
  const fw = () => {
    const s = boardStatus();
    if (!s?.firmware_sha) return "";
    return s.firmware_dirty ? `${s.firmware_sha}*` : s.firmware_sha;
  };

  return (
    <header class="topbar">
      <A href="/" class="brand">
        <span class="brand__leds" aria-hidden="true">
          <For each={Array.from({ length: 8 })}>{() => <i />}</For>
        </span>
        LED Board
      </A>

      <nav class="tabs">
        <For each={props.routes}>
          {(route) => (
            <A href={route.path} class={`tab ${isSelected(route.path) ? "tab--selected" : ""}`}>
              {route.label}
            </A>
          )}
        </For>
      </nav>

      <div class="topbar__right">
        <Show when={fw()}>
          <span
            class="faint"
            style={{ "font-size": "11.5px", "font-family": "ui-monospace, SFMono-Regular, Menlo, monospace" }}
            title={`firmware ${boardStatus()?.firmware_sha}${boardStatus()?.firmware_dirty ? " (dirty tree)" : ""}`}
          >
            {fw()}
          </span>
        </Show>
        <span
          class={`conn-dot ${connected() ? "conn-dot--live" : "conn-dot--retry"}`}
          title={connected() ? "Connected — live status feed" : "Reconnecting to the board…"}
        />
        <span class="faint" style={{ "font-size": "11.5px" }}>
          {connected() ? "live" : "offline"}
        </span>
      </div>
    </header>
  );
}
