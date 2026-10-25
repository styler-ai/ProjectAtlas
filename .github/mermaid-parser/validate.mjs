let source = "";
for await (const chunk of process.stdin) {
  source += chunk;
}

let JSDOM;
let mermaid;
let bootstrapReady = false;
try {
  ({ JSDOM } = await import("jsdom"));
  const dom = new JSDOM("<!doctype html><html><body></body></html>");
  globalThis.window = dom.window;
  globalThis.document = dom.window.document;
  globalThis.Option = dom.window.Option;
  ({ default: mermaid } = await import("mermaid"));
  mermaid.initialize({ startOnLoad: false });
  bootstrapReady = true;
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 2;
}

if (bootstrapReady) {
  try {
    await mermaid.parse(source);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
