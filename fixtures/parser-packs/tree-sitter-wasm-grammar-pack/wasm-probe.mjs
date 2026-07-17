import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const [modulePath] = process.argv.slice(2);
if (modulePath === undefined) {
  throw new Error("usage: node wasm-probe.mjs <module.wasm>");
}

const bytes = readFileSync(modulePath);
const module = new WebAssembly.Module(bytes);
const memory = new WebAssembly.Memory({ initial: 1024, maximum: 1024 });
const table = new WebAssembly.Table({ initial: 1024, element: "anyfunc" });
const env = {
  iswspace: (codePoint) => (codePoint === 32 ? 1 : 0),
  iswalpha: (codePoint) =>
    (codePoint >= 65 && codePoint <= 90) ||
    (codePoint >= 97 && codePoint <= 122)
      ? 1
      : 0,
  __stack_pointer: new WebAssembly.Global(
    { value: "i32", mutable: true },
    67_108_848,
  ),
  __memory_base: new WebAssembly.Global({ value: "i32" }, 1024),
  __table_base: new WebAssembly.Global({ value: "i32" }, 0),
  memory,
  __indirect_function_table: table,
};
const instance = new WebAssembly.Instance(module, { env });
instance.exports.__wasm_apply_data_relocs();
instance.exports.__wasm_call_ctors();
const pointer = instance.exports.tree_sitter_javascript();
const abi = new DataView(memory.buffer).getUint32(pointer, true);
const result = {
  module_sha256: createHash("sha256").update(bytes).digest("hex"),
  pointer,
  abi,
  exports: WebAssembly.Module.exports(module).map(({ name }) => name),
};
process.stdout.write(JSON.stringify(result));
