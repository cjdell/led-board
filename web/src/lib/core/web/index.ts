import { RouteSectionProps } from "@solidjs/router";
import { createSignal, JSXElement } from "solid-js";
import * as v from "valibot";
import { assertError } from "../utils/index.ts";

export interface RouteDefinition {
  label: string;
  path: string;
  component: (props: RouteSectionProps) => JSXElement;
}

export interface FieldMetadata {
  [key: string]: unknown;
  password?: boolean;
}

export const FieldMetadata = (m: FieldMetadata) => m;

// ── Toasts ────────────────────────────────────────────────────────────────

export type ToastKind = "info" | "ok" | "warn" | "error";

export interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
}

const [toasts, setToasts] = createSignal<Toast[]>([], { equals: false });

let nextToastId = 1;

export function toast(message: string, kind: ToastKind = "info", ttl = 4_000) {
  const entry: Toast = { id: nextToastId++, kind, message };

  setToasts((list) => [...list, entry]);

  setTimeout(() => dismissToast(entry.id), ttl);
}

export function dismissToast(id: number) {
  setToasts((list) => list.filter((t) => t.id !== id));
}

export { toasts };

/// Human-readable message for anything thrown (valibot errors included).
export function errorMessage(err: unknown): string {
  if (err instanceof v.ValiError) {
    return err.issues.map((i, idx) => `[${idx}] ${i.message}`).join("\n");
  }

  if (err instanceof Error) return err.message;

  return String(err);
}

/// Wrap an async UI handler: errors hit the toast stack, never a modal alert.
export function guardAsync<A extends unknown[]>(fn: (...args: A) => Promise<void>): (...args: A) => Promise<void> {
  return async (...args: A) => {
    try {
      await fn(...args);
    } catch (err) {
      assertError(err);
      console.error(err);
      toast(errorMessage(err), "error", 6_000);
    }
  };
}
