<script lang="ts">
  // Light smoke-check of the IPC wiring: ping → pong. Real UI arrives with
  // the library grid tasks (01-013+).
  let pong = $state<string | null>(null);

  async function ping(): Promise<void> {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      pong = await invoke<string>("ping");
    } catch {
      pong = "no backend (browser dev)";
    }
  }
</script>

<main>
  <h1>Fotobase</h1>
  <p>Local-first photo organizer — skeleton.</p>
  <button onclick={ping}>Ping backend</button>
  {#if pong}
    <p>Backend says: <code>{pong}</code></p>
  {/if}
</main>
