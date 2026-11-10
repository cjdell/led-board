import { For } from "solid-js";
import { dismissToast, toasts } from "@lib";

export function Toasts() {
  return (
    <div
      class="toast-stack"
      onClick={(e) => {
        const target = e.target as HTMLElement;
        if (target.classList.contains("toast")) {
          dismissToast(Number(target.dataset["id"]));
        }
      }}
    >
      <For each={toasts()}>
        {(toast) => (
          <div class={`toast toast--${toast.kind}`} data-id={toast.id}>
            {toast.message}
          </div>
        )}
      </For>
    </div>
  );
}
