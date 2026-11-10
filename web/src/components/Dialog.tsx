import type { JSX } from "solid-js";
import { Show } from "solid-js";

interface Props {
  open: boolean;
  title: string;
  onClose: () => void;
  onConfirm?: () => void;
  confirmLabel?: string;
  children: JSX.Element;
}

/// Small modal used for prompts ("Save playlist as…", confirmations).
/// Escape/ backdrop click close it.
export function Dialog(props: Props) {
  return (
    <Show when={props.open}>
      <div
        class="dialog-backdrop"
        onKeyDown={(e) => e.key === "Escape" && props.onClose()}
        onClick={(e) => e.target === e.currentTarget && props.onClose()}
      >
        <div class="dialog" role="dialog" aria-modal="true" aria-label={props.title}>
          <header class="panel__hd">{props.title}</header>
          <div class="panel__bd">{props.children}</div>
          <footer class="panel__ft">
            <button class="btn btn--ghost" onClick={props.onClose}>
              Cancel
            </button>
            <Show when={props.onConfirm}>
              <button
                class="btn btn--primary"
                onClick={() => {
                  props.onConfirm!();
                  props.onClose();
                }}
              >
                {props.confirmLabel ?? "OK"}
              </button>
            </Show>
          </footer>
        </div>
      </div>
    </Show>
  );
}
