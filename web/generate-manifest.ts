import path from "node:path";

const WasmPath = path.join(import.meta.dirname, "public/wasm");

const files = Deno.readDirSync(WasmPath);

const manifest: { name: string; size: number }[] = [];

for (const file of files) {
  if (file.name.endsWith(".txt")) {
    manifest.push({
      name: file.name,
      size: Deno.statSync(path.join(WasmPath, file.name)).size,
    });
  }
}

Deno.writeTextFileSync(path.join(WasmPath, "manifest.json"), JSON.stringify(manifest));
