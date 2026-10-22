// Empty Svelte config on purpose (vite-plugin-svelte defaults apply).
//
// The Svelte VS Code extension's language server (svelte-check ≥ 4.6,
// @sveltejs/load-config ≥ 0.2) resolves the Svelte options from vite.config,
// but its loader matches the plugin-name layout of vite-plugin-svelte v6
// (`vite-plugin-svelte:config`). This project pins v5 (main plugin is named
// `vite-plugin-svelte`), so the vite path resolves to "no svelte plugin" and
// the extension logs "No Svelte configuration found in vite config".
//
// An explicit svelte.config.js takes over as the config source for the
// language server (`configSource: 'svelte'`) and silences that error, while
// staying a no-op for the build: vite-plugin-svelte treats an empty config
// exactly like a missing one.
export default {};
