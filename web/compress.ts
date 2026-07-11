await Deno.mkdir("bundle", { recursive: true });

const htmlFile = "dist/index.html";

const html = Deno.readTextFileSync(htmlFile);

// Replace with inline  and <style> tags.
const inlinedHtml = inlineExternalReferences(html);

Deno.writeTextFileSync("bundle/index.html", inlinedHtml);

// Apply GZIP compression so the server can just reply with the raw bytes.
await compressWithGzip();

// Function to inline external script and stylesheet references
function inlineExternalReferences(html: string) {
  // Pattern to match <script> tags with src attribute
  const scriptRegex = /<script[^>]+src=["']([^"']+)["'][^>]*><\/script>/g;

  // Pattern to match <link> tags with rel="stylesheet" and href attribute
  const linkRegex = /<link[^>]+rel=["']stylesheet["'][^>]+href=["']([^"']+)["'][^>]*\/?>/g;

  // Replace script tags
  let result = html.replace(scriptRegex, (match, src) => {
    try {
      // Remove leading slash if present (relative to root)
      const filePath = src.startsWith("/") ? src.substring(1) : src;
      const scriptContent = Deno.readTextFileSync(`dist/${filePath}`);
      return `<script type="module" crossorigin>${scriptContent}</script>`;
    } catch (error) {
      // If file doesn't exist, keep the original tag
      console.warn(`Could not find script file: ${src}`, error);
      return match;
    }
  });

  // Replace link tags with stylesheet content
  result = result.replace(linkRegex, (match, href) => {
    try {
      // Remove leading slash if present (relative to root)
      const filePath = href.startsWith("/") ? href.substring(1) : href;
      const stylesheetContent = Deno.readTextFileSync(`dist/${filePath}`);
      return `<style type="text/css">${stylesheetContent}</style>`;
    } catch (error) {
      // If file doesn't exist, keep the original tag
      console.warn(`Could not find stylesheet file: ${href}`, error);
      return match;
    }
  });

  return result;
}

// Function to compress data with GZIP
async function compressWithGzip() {
  const input = await Deno.open("bundle/index.html");
  const output = await Deno.open("bundle/index.html.gz", { create: true, write: true });

  await input.readable
    .pipeThrough(new CompressionStream("gzip"))
    .pipeTo(output.writable);
}
