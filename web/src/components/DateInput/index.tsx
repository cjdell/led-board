import { fromIsoDate, IsoDateSchema, toIsoDate } from "@lib";
import { format, parse } from "date-fns";
import { enGB } from "date-fns/locale";
import { createMemo, createSignal, onMount } from "solid-js";
import { assert } from "ts-essentials";
import * as v from "valibot";
import { DatePicker } from "../DatePicker/index.tsx";
import { openAlert } from "@lib";

interface Props {
  id: string;
  isInvalid: boolean;
  placeholder: string;
  readonly: boolean;
  value: string | null | undefined;

  onChange: (value: string | undefined) => void;
}

export function DateInput(props: Props) {
  let inputRef: HTMLInputElement | null = null;

  const [isOpen, setIsOpen] = createSignal(false);

  const date = createMemo(() => {
    const parseResult = v.safeParse(IsoDateSchema, props.value);
    return parseResult.success ? fromIsoDate(parseResult.output) : undefined;
  });

  onMount(() => {
    const onClickOutside = (e: PointerEvent) => {
      assert(e.target instanceof Element, "Not an element!");
      if (e.target !== inputRef && !e.target.closest(".date-picker")) {
        setIsOpen(false);
      }
    };

    document.addEventListener("click", onClickOutside);
    return () => document.removeEventListener("click", onClickOutside);
  });

  const onInputClick = () => {
    setIsOpen(!isOpen());
  };

  const onDatePickerChange = (date: Date) => {
    props.onChange(toIsoDate(date));
    setIsOpen(false);
  };

  const asLocaleString = () => {
    const parseResult = v.safeParse(IsoDateSchema, props.value);
    return parseResult.success ? format(parseResult.output, "PPP", { locale: enGB }) : "";
  };

  const fromLocaleString = (str: string) => {
    const date = parse(str, "PPP", new Date(), { locale: enGB });
    if (date instanceof Date && !isNaN(date.getFullYear())) {
      props.onChange(toIsoDate(new Date(Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()))));
    } else {
      openAlert("Date Format Error", `Could not parse date string "${str}".`);
    }
  };

  return (
    <>
      <div class="date-input-date-picker-container" style={{ visibility: isOpen() ? "visible" : "hidden" }}>
        <DatePicker
          value={date()}
          onChange={onDatePickerChange}
        />
      </div>

      <input
        ref={(el) => inputRef = el}
        type="text"
        id={props.id}
        classList={{
          "form-control": true,
          "is-invalid": props.isInvalid,
          "value-null": props.value === null,
          "value-undefined": props.value === undefined,
        }}
        placeholder={props.placeholder}
        value={asLocaleString()}
        on:change={(e) => fromLocaleString(e.currentTarget.value)}
        on:click={onInputClick}
      />
    </>
  );
}
