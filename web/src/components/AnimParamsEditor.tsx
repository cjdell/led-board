import { createSignal, For, Show } from "solid-js";
import { type Animation, animationKind, type AnimationParams } from "@lib";

interface Props {
  animation: Animation;
  onChange: (animation: Animation) => void;
}

type Primitive = string | number | boolean | null;

interface ParamObject {
  [key: string]: ParamValue;
}

type ParamValue = Primitive | ParamValue[] | ParamObject;

/// Inline editor for parameterised animations (`CenteredTitle`, `…Message`)
/// — recursive over the JSON shape, primitives only, no schema needed:
/// the board's `/api/info/animations` samples are already the values.
export function AnimParamsEditor(props: Props) {
  const kind = () => animationKind(props.animation);

  const params = (): ParamValue => typeof props.animation === "string" ? null : (Object.values(props.animation)[0] as ParamValue);

  const emit = (next: ParamValue) => {
    if (next === null || next === undefined) {
      props.onChange(kind());
    } else {
      props.onChange({ [kind()]: next as AnimationParams });
    }
  };

  const [open, setOpen] = createSignal(false);

  return (
    <Show when={params() !== null && params() !== undefined} fallback={<span />}>
      <button class="btn btn--ghost" style={{ padding: "4px 8px", "font-size": "12px" }} onClick={() => setOpen(!open())}>
        {open() ? "▾ params" : "▸ params"}
      </button>

      <Show when={open()}>
        <div class="vstack" style={{ padding: "6px 0 2px" }}>
          <ParamField value={params()!} onChange={emit} />
        </div>
      </Show>
    </Show>
  );
}

interface FieldProps {
  value: ParamValue;
  onChange: (value: ParamValue) => void;
}

function ParamField(props: FieldProps) {
  const value = () => props.value;

  const setChild = (index: number, v: ParamValue) => {
    const next = [...(props.value as ParamValue[])];

    next[index] = v;
    props.onChange(next);
  };

  return (
    <Show
      when={typeof value() !== "object" || value() === null}
      fallback={Array.isArray(value())
        ? (
          <div class="vstack" style={{ gap: "6px" }}>
            <For each={value() as ParamValue[]}>{(item, i) => <ParamField value={item} onChange={(v) => setChild(i(), v)} />}</For>
          </div>
        )
        : (
          <div class="vstack" style={{ gap: "6px" }}>
            <For
              each={Object.entries(value() as Record<string, ParamValue>)}
            >
              {([key, item]) => (
                <div class="field">
                  <label>{key}</label>
                  <ParamField
                    value={item}
                    onChange={(v) => props.onChange({ ...(props.value as Record<string, ParamValue>), [key]: v })}
                  />
                </div>
              )}
            </For>
          </div>
        )}
    >
      {(() => {
        const primitive = value() as string | number | boolean | null;

        if (typeof primitive === "boolean") {
          return (
            <label class="check">
              <input type="checkbox" checked={primitive} onChange={(e) => props.onChange(e.currentTarget.checked)} />
              primitive
            </label>
          );
        }

        if (typeof primitive === "number") {
          return (
            <input
              class="input input--number"
              type="number"
              value={primitive}
              onChange={(e) => props.onChange(Number(e.currentTarget.value))}
            />
          );
        }

        return (
          <input
            class="input"
            type="text"
            value={primitive === null ? "" : String(primitive)}
            onChange={(e) => props.onChange(e.currentTarget.value)}
          />
        );
      })()}
    </Show>
  );
}
