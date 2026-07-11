import { For, Show } from "solid-js";
import type { ElementOf } from "ts-essentials";

interface Props<TOptions extends readonly SelectOption[]> {
  id: string;
  isInvalid: boolean;
  placeholder: string;
  value: ElementOf<TOptions>["value"] | undefined | null;
  options: TOptions;
  allowNull: boolean;

  onChange: (value: string | undefined | null) => void;
}

export interface SelectOption {
  value: string;
  text: string;
}

export function Select<TOptions extends readonly SelectOption[]>(props: Props<TOptions>) {
  return (
    <select
      value={props.value === undefined ? "[undefined]" : props.value === null ? ["null"] : props.value}
      classList={{
        "form-control": true,
        "is-invalid": props.isInvalid,
        "value-undefined": props.value === undefined,
        "value-null": props.value === null,
      }}
      title={props.placeholder}
      on:change={(e) =>
        props.onChange(
          e.currentTarget.value === "[undefined]" ? undefined : e.currentTarget.value === "[null]" ? null : e.currentTarget.value,
        )}
    >
      <Show when={props.value === undefined}>
        <option value="[undefined]">(Select {props.placeholder})</option>
      </Show>
      <Show when={props.allowNull}>
        <option value="[null]">(Empty)</option>
      </Show>
      <For each={props.options}>{(o) => <option value={o.value}>{o.text}</option>}</For>
    </select>
  );
}
