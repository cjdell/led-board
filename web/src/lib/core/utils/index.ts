export function assertError(err: unknown): asserts err is Error {
  if (!(err instanceof Error)) {
    throw new Error(`Error is not an instance of Error: ${String(err)}`);
  }
}

export function assertUnreachable(x: never): never {
  console.error("assertUnreachable:", x);

  throw new Error(`An unreachable event has occurred: ${String(x)} / ${typeof x}`);
}

export function humanise(inputString: string) {
  const formattedString = inputString.replace(/[-_]/g, " ");

  const finalFormattedString = formattedString.replace(/([a-z])([A-Z])/g, "$1 $2");

  return finalFormattedString.replace(/\b\w/g, (match) => match.toUpperCase());
}

export const isNotNull = <T>(t: T): t is Exclude<T, null> => {
  return t !== null;
};

export const isNotNullOrUndefined = <T>(t: T): t is Exclude<T, null | undefined> => {
  return t !== null && t !== undefined;
};

export function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/// `95_000 → "1:35"`, `3_600_000 + … → "1h 00m"`.
export function formatDuration(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));

  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`;

  return `${Math.floor(s / 3600)}h ${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}m`;
}

/// `12345 → "3h 25m"` style uptime.
export function formatUptime(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;

  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);

  if (h < 24) return `${h}h ${String(m).padStart(2, "0")}m`;

  return `${Math.floor(h / 24)}d ${h % 24}h`;
}

export function downloadText(filename: string, text: string, type = "application/json") {
  const blob = new Blob([text], { type });
  const url = URL.createObjectURL(blob);

  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

/// Native file picker resolving to the chosen file's text (playlist import).
export function pickTextFile(): Promise<string> {
  return new Promise((resolve, reject) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".json,application/json";

    input.addEventListener("change", () => {
      const file = input.files?.[0];

      if (!file) return reject(new Error("No file chosen"));

      const reader = new FileReader();

      reader.addEventListener("loadend", () => {
        if (typeof reader.result === "string") resolve(reader.result);
        else reject(new Error("Could not read file"));
      });

      reader.readAsText(file);
    });

    input.click();
  });
}
