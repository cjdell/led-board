// deno-lint-ignore-file no-explicit-any
import { Button, MagicFields } from "@components";
import { isArray } from "@lib";
import { For } from "solid-js";
import * as v from "valibot";

interface Props {
  items?: Array<unknown>;
  itemSchema: v.ObjectSchema<any, any>;
  validation: boolean;

  onChange: (items: Array<unknown>) => void;
}

export function NestedMagicFields(props: Props) {
  if (!isArray(props.items)) {
    return "Not an array";
  }

  return (
    <div class="NestedMagicFields g-col-12 border p-3">
      <div class="grid gap-3">
        <For each={props.items} fallback={<div class="g-col-12">No Wifi networks saved. Please click "Scan".</div>}>
          {(item, idx) => (
            <div class="g-col-12">
              <MagicFields
                formFieldClass="g-col-6"
                schema={props.itemSchema}
                data={item as any}
                validation={props.validation}
                onChange={(changedItem) => props.onChange(props.items!.map((_item, _idx) => _idx !== idx() ? _item : changedItem))}
              />

              <Button colour="danger" class="mt-2" on:click={() => props.onChange(props.items!.filter((_, _idx) => _idx !== idx()))}>
                Delete
              </Button>
            </div>
          )}
        </For>
      </div>
    </div>
  );
}
