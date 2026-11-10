import { For } from "solid-js";

/// 4-bar RSSI meter: > -60 excellent, > -70 good, > -80 fair, else poor.
export function SignalMeter(props: { rssi: number }) {
  const bars = () => (props.rssi >= -60 ? 4 : props.rssi >= -70 ? 3 : props.rssi >= -80 ? 2 : 1);

  return (
    <span class="signal-meter" title={`${props.rssi} dBm`} aria-label={`signal ${bars()} of 4`}>
      <For each={[1, 2, 3, 4]}>
        {(bar) => <i class={bar <= bars() ? "on" : ""} style={{ height: `${bar * 3 + 3}px` }} />}
      </For>
    </span>
  );
}
