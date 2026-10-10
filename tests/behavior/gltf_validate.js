// Runs the Khronos glTF validator on files and prints one JSON line per file.
// Install the package next to this script first: npm install gltf-validator
const validator = require('gltf-validator');
const fs = require('fs');
(async () => {
  for (const f of process.argv.slice(2)) {
    const bytes = new Uint8Array(fs.readFileSync(f));
    const r = await validator.validateBytes(bytes, {
      uri: f,
      externalResourceFunction: (uri) => Promise.reject(new Error('no external: ' + uri)),
    });
    console.log(JSON.stringify({
      file: f,
      errors: r.issues.numErrors,
      warnings: r.issues.numWarnings,
      messages: r.issues.messages.map((m) => ({ severity: m.severity, code: m.code, pointer: m.pointer })),
    }));
  }
})();
