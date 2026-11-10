import { For } from "solid-js";
import { A } from "@solidjs/router";
import { connected, type RouteDefinition } from "@lib";

interface Props {
  routes: readonly RouteDefinition[];
  pathname: string;
}

export function TopBar(props: Props) {
  const isSelected = (path: string) => path === "/" ? props.pathname === "/" : props.pathname.startsWith(path);

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
