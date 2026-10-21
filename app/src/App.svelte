<script lang="ts">
  // Skeleton shell (library grid arrives with 01-013+). This carries the
  // first wiring of the Options dialog (TASK-01-022): settings load at
  // startup, the dialog persists every change, and the update-mode policy
  // runs once on startup.
  import OptionsDialog from "./lib/OptionsDialog.svelte";
  import { setLocale, t } from "./lib/i18n.svelte.js";
  import { loadSettings, type Settings } from "./lib/settings";
  import { runUpdateFlow } from "./lib/updates";

  let pong = $state<string | null>(null);
  let optionsOpen = $state(false);
  let settings = $state<Settings | null>(null);

  async function ping(): Promise<void> {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      pong = await invoke<string>("ping");
    } catch {
      pong = t("app.backendMissing");
    }
  }

  $effect(() => {
    // Startup: load persisted settings, apply the language, then honor the
    // update policy (auto installs, check-but-ask prompts, off does
    // nothing — the backend gate enforces that too).
    void (async () => {
      const s = await loadSettings();
      settings = s;
      setLocale(s.language);
      await runUpdateFlow(s.update_mode);
    })();
  });
</script>

<main>
  <h1>{t("app.title")}</h1>
  <p>{t("app.tagline")}</p>
  <button
    type="button"
    onclick={ping}
    title={settings?.show_tooltips ? t("app.ping.tooltip") : undefined}
  >
    {t("app.ping")}
  </button>
  {#if pong}
    <p>{t("app.backendSays")} <code>{pong}</code></p>
  {/if}
  <button type="button" onclick={() => (optionsOpen = true)} data-testid="open-options">
    {t("app.options")}
  </button>
  {#if settings}
    <OptionsDialog
      bind:open={optionsOpen}
      settings={settings}
      onchange={(s) => {
        settings = s;
        setLocale(s.language);
      }}
    />
  {/if}
</main>
