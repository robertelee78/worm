// Module customization hooks for the app.js smoke test: redirect app.js's
// `./pkg/worm.js` import (the wasm-bindgen bundle — unrunnable in node) to
// the controllable stub in ./wasm-stub.mjs. Registered via module.register()
// at the top of app-smoke.mjs, before app.js is imported.
export async function resolve(specifier, context, next) {
  if (
    specifier === './pkg/worm.js' &&
    context.parentURL &&
    context.parentURL.endsWith('/web/app.js')
  ) {
    return { url: new URL('./wasm-stub.mjs', import.meta.url).href, shortCircuit: true };
  }
  return next(specifier, context);
}
