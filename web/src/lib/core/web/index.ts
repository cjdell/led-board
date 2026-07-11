// deno-lint-ignore-file no-explicit-any
import { RouteSectionProps } from "@solidjs/router";
import { Accessor, JSXElement } from "solid-js";
import type { ElementOf } from "ts-essentials";
import { assert } from "ts-essentials";
import * as v from "valibot";
import { assertError, humanise } from "../utils/index.ts";

export interface RouteDefinition {
  label: string;
  path: string;
  component: (props: RouteSectionProps) => JSXElement;
}

export interface SelectOption {
  value: string;
  text: string;
}

export const Colours = ["default", "primary", "secondary", "success", "danger", "warning", "info"] as const;

export type Colour = ElementOf<typeof Colours>;

export interface QuerySort {
  sort: string;
  dir: "asc" | "desc";
}

export interface FetchParameters {
  skip: number;
  take: number;
  orderBy: (readonly [string, "asc" | "desc"])[];
}

export interface FieldMetadata {
  [key: string]: unknown;
  icon?: string;
  lookup?: string;
  displayMode?: string;
  width?: string;
  text?: boolean;
  readonly?: boolean;
}

export const FieldMetadata = (m: FieldMetadata) => m;

// Allows type guards to work within a template
export const narrow = <A, B extends A>(accessor: Accessor<A>, guard: (v: A) => v is B): B | null => {
  const val = accessor();
  if (guard(val)) {
    return val;
  }
  return null;
};

// deno-lint-ignore require-await
export async function openAlert(title: string, message: string) {
  alert(`${title}\n\n${message}`);
}

/** Get the closest ancestor that is scrolling this element (overflow/overflow-y) */
export function getScrollingAncestor(el: HTMLElement): HTMLElement | undefined {
  while (el && el.parentElement) {
    const style = getComputedStyle(el);
    if (/(auto|scroll)/.test(style.overflowY || style.overflow)) {
      return el;
    }
    el = el.parentElement;
  }
  return undefined;
}

export function debounce<
  TFunc extends (...args: TArgs) => void,
  TArgs extends unknown[],
>(
  callback: TFunc,
  wait: number,
) {
  let lastCallTime = 0;
  let timeout: ReturnType<typeof setTimeout> | null = null;

  return (...args: TArgs) => {
    const now = Date.now();

    if (now - lastCallTime > wait) {
      // If enough time has passed, call immediately
      lastCallTime = now;
      callback(...args);
    } else {
      // Otherwise, clear existing timeout and set a new one
      if (timeout) {
        clearTimeout(timeout);
      }
      timeout = setTimeout(() => {
        lastCallTime = Date.now();
        callback(...args);
      }, wait);
    }
  };
}

function isSelfClickable(target: EventTarget | null) {
  // Allow clicking child elements
  if (target instanceof HTMLInputElement) return true;
  if (target instanceof HTMLButtonElement) return true;

  if (target instanceof HTMLElement) {
    if (target.classList.contains("clickable")) return true;
  }

  return false;
}

export const createLongPressHandler = (
  { onShortTap, onLongTap }: { onShortTap: (e: MouseEvent | TouchEvent) => void; onLongTap: () => void },
) => {
  let timer: number | null = null;
  let startPos: { x: number; y: number } | null = null;
  let isLongPress = false;
  let hasMoved = false;

  const cleanup = () => {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    startPos = null;
    isLongPress = false;
    hasMoved = false;
  };

  const triggerHaptic = () => {
    if ("vibrate" in navigator) {
      navigator.vibrate(50);
    }
  };

  return {
    onMouseDown: (e: MouseEvent) => {
      if (isSelfClickable(e.target)) return;

      startPos = { x: e.clientX, y: e.clientY };
      isLongPress = false;
      hasMoved = false;

      timer = setTimeout(() => {
        if (timer && !hasMoved) {
          isLongPress = true;
          triggerHaptic();
          onLongTap();
        }
        cleanup();
      }, 500);
    },

    onMouseMove: (e: MouseEvent) => {
      if (isSelfClickable(e.target)) return;

      if (!startPos || !timer) return;

      const distance = Math.sqrt(Math.pow(e.clientX - startPos.x, 2) + Math.pow(e.clientY - startPos.y, 2));

      if (distance > 10) {
        hasMoved = true;
        cleanup();
      }
    },

    onMouseUp: (e: MouseEvent) => {
      if (isSelfClickable(e.target)) return;

      // Only prevent default for short taps (not scrolls)
      if (timer && !hasMoved) {
        e.preventDefault(); // Prevent text selection only for actual taps

        if (!isLongPress) {
          onShortTap(e);
        }
      }
      cleanup();
    },

    onTouchStart: {
      handleEvent: (e: TouchEvent) => {
        const touch = e.touches[0];
        startPos = { x: touch.clientX, y: touch.clientY };
        isLongPress = false;
        hasMoved = false;

        timer = setTimeout(() => {
          if (timer && !hasMoved) {
            isLongPress = true;
            triggerHaptic();
            onLongTap();
          }
          cleanup();
        }, 500);
      },
      passive: true,
    },

    onTouchMove: {
      handleEvent: (e: TouchEvent) => {
        if (!startPos || !timer) return;

        const touch = e.touches[0];
        const distance = Math.sqrt(Math.pow(touch.clientX - startPos.x, 2) + Math.pow(touch.clientY - startPos.y, 2));

        if (distance > 10) {
          hasMoved = true;
          cleanup();
        }
      },
      passive: true,
    },

    onTouchEnd: (e: TouchEvent) => {
      // Only prevent default for short taps (not scrolls)
      if (timer && !hasMoved) {
        e.preventDefault(); // Prevent text selection only for actual taps

        if (!isLongPress) {
          onShortTap(e);
        }
      }
      cleanup();
    },

    onTouchCancel: cleanup,
  };
};

export function normaliseError(err: Error) {
  return err;
}

export function handleAsyncClick(
  handle: (e: MouseEvent | TouchEvent) => Promise<void>,
  setWorking: (working: boolean) => void,
) {
  return async (e: MouseEvent | TouchEvent) => {
    try {
      setWorking(true);
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      await handle(e);
    } catch (_err) {
      assertError(_err);

      console.log("Error", _err.constructor.name);
      console.error(_err);

      const err = normaliseError(_err);

      let message = err.message;

      if (err instanceof v.ValiError) {
        message = err.issues.map((i, idx) => `[${idx}] ${i.message}`).join(
          "\n",
        );
      }

      await openAlert("An error occurred", message);
    } finally {
      setWorking(false);
    }
  };
}

export function getFieldInfo(formSchema: v.ObjectSchema<any, any>, fieldName: string) {
  const maybePropSchema = formSchema.entries[fieldName] as
    | v.BaseSchema<any, any, any>
    | v.NullableSchema<any, any>
    | v.OptionalSchema<any, any>
    | v.SchemaWithPipe<Array<any> & [any]>;

  let propSchema = maybePropSchema;

  let nullable = false;
  let optional = false;

  // Keep unwrapping until we have the actual schema...
  while ("wrapped" in propSchema) {
    if ("type" in propSchema) {
      if (propSchema.type === "nullable") {
        nullable = true;
      }

      if (propSchema.type === "optional") {
        optional = true;
      }
    }

    propSchema = propSchema.wrapped as v.SchemaWithPipe<Array<any> & [any]>;
  }

  const pipe = "pipe" in propSchema ? propSchema.pipe : [];

  const typeSchema = pipe.find((item): item is v.BaseSchema<any, any, any> => item.kind === "schema") ?? propSchema;

  const type = typeSchema?.type;

  const validationTypes = pipe.filter(
    (item): item is v.BaseValidation<any, any, any> => item.kind === "validation",
  ).map((item) => item.type);

  const title = pipe.find((item): item is v.TitleAction<string, string> => item.type === "title")?.title ?? humanise(fieldName);

  const description: string | undefined = pipe.find(
    (item): item is v.DescriptionAction<string, string> => item.type === "description",
  )?.description;

  const metadata = pipe.find(
    (item): item is v.MetadataAction<string, FieldMetadata> => item.type === "metadata",
  )?.metadata;

  let inputType: "text" | "select" | "email" | "password" | "lookup" | "textarea" | "date" | "datetime" | "array" = "text";

  let options: SelectOption[] = [];

  let entityType: string | undefined;

  let arrayItemSchema: v.ObjectSchema<any, any> | undefined;

  if (metadata?.lookup) {
    inputType = "lookup";
    entityType = metadata.lookup;
  } else if (type === "picklist") {
    inputType = "select";

    options = (typeSchema as v.PicklistSchema<any, any>).options.map((o: string) => ({
      value: o,
      text: humanise(o),
    }));
  } else if (type === "date") {
    inputType = "datetime";
  } else if (type === "string") {
    if (validationTypes.includes("email")) {
      inputType = "email";
    } else if (validationTypes.includes("iso_date")) {
      inputType = "date";
    }

    if (title.toLowerCase().includes("password")) {
      inputType = "password";
    }
  } else if (type === "array") {
    inputType = "array";

    const arraySchema = propSchema as v.ArraySchema<any, any>;
    assert(arraySchema.type === "array", "Not an array schema!");

    arrayItemSchema = arraySchema.item;
    assert(arrayItemSchema?.type === "object", "Not an object schema!");
  } else {
    throw new Error(`Unknown type: ${type}`);
  }

  if (metadata?.text) {
    inputType = "textarea";
  }

  return { metadata, title, inputType, options, description, entityType, arrayItemSchema, nullable, optional };
}

export function downloadFile(file_name: string, file_data: Uint8Array) {
  // Create a blob from the Uint8Array data
  const blob = new Blob([file_data.buffer as ArrayBuffer], { type: "application/octet-binary" }); // Change the type if needed
  const url = URL.createObjectURL(blob);

  // Create a link to the blob URL and click it
  const a = document.createElement("a");
  a.href = url;
  a.download = file_name;
  a.click(); // This will trigger the download
  URL.revokeObjectURL(url); // Clean up
}

export function uploadFile() {
  return new Promise<[string, Uint8Array]>((resolve, reject) => {
    const input = document.createElement("input");
    input.type = "file";
    input.addEventListener("change", (e) => {
      if (!input.files?.length) return reject(new Error("No file!"));
      const reader = new FileReader();
      const file = input.files[0];
      reader.readAsArrayBuffer(file);
      reader.addEventListener("loadend", () => {
        if (reader.result) {
          return resolve([file.name, new Uint8Array(reader.result as ArrayBuffer)]);
        } else {
          return reject("Could not read file!");
        }
      });
    });
    input.click();
  });
}
