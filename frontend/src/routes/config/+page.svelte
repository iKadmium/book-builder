<script lang="ts">
  import { AppBar } from "@skeletonlabs/skeleton-svelte";
  import type { AppConfig } from "$lib/types";
  import type { PageData } from "./$types";
  import { untrack } from "svelte";
  import { ensureOk, errorMessage } from "$lib/errors";

  let { data }: { data: PageData } = $props();

  // Deep clone so edits don't mutate the load data directly
  let cfg: AppConfig = $state(untrack(() => structuredClone(data.config)));
  let saving = $state(false);
  let saved = $state(false);
  let error = $state("");

  async function save() {
    saving = true;
    saved = false;
    error = "";
    try {
      const res = await fetch("/api/config", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(cfg),
      });
      await ensureOk(res);
      saved = true;
      setTimeout(() => (saved = false), 3000);
    } catch (cause) {
      error = `Save failed: ${errorMessage(cause)}`;
    } finally {
      saving = false;
    }
  }
</script>

<div class="flex flex-col min-h-screen">
  <AppBar>
    <AppBar.Toolbar>
      <AppBar.Lead>
        <a href="/" class="text-xl font-bold hover:opacity-80">Book Builder</a>
      </AppBar.Lead>
      <AppBar.Trail>
        <a href="/" class="btn preset-tonal text-sm">← Back</a>
      </AppBar.Trail>
    </AppBar.Toolbar>
  </AppBar>

  <main class="container mx-auto p-8 max-w-2xl space-y-8">
    <h1 class="h2">Configuration</h1>
    {#if data.loadError}
      <div
        role="alert"
        class="border-l-4 border-error-500 p-3 text-error-500 whitespace-pre-wrap break-words"
      >
        {data.loadError}
      </div>
    {/if}
    {#if data.oauthError}
      <div
        role="alert"
        class="border-l-4 border-error-500 p-3 text-error-500 whitespace-pre-wrap break-words"
      >
        OAuth failed: {data.oauthError}
      </div>
    {/if}

    <!-- Forgejo -->
    <section class="card preset-filled-surface-100-900 p-6 space-y-4">
      <h2 class="h3">Forgejo</h2>
      <label class="label">
        <span>Instance URL</span>
        <input
          class="input"
          type="url"
          bind:value={cfg.forgejo.url}
          placeholder="http://git.example.com"
        />
      </label>
      <label class="label">
        <span>Repository (owner/repo)</span>
        <input
          class="input"
          type="text"
          bind:value={cfg.forgejo.repo}
          placeholder="owner/books"
        />
      </label>
      <p class="text-sm opacity-60">
        OAuth client credentials are configured via <code
          >FORGEJO_CLIENT_ID</code
        >
        and
        <code>FORGEJO_CLIENT_SECRET</code> environment variables.
      </p>
      <a href="/api/oauth/forgejo/authorize" class="btn preset-tonal text-sm">
        Connect to Forgejo
      </a>
    </section>

    <!-- Google -->
    <section class="card preset-filled-surface-100-900 p-6 space-y-4">
      <h2 class="h3">Google</h2>
      <p class="text-sm opacity-60">
        OAuth client credentials are configured via <code>GOOGLE_CLIENT_ID</code
        >
        and
        <code>GOOGLE_CLIENT_SECRET</code> environment variables.
      </p>
      <a href="/api/oauth/google/authorize" class="btn preset-tonal text-sm">
        Connect to Google
      </a>
    </section>

    <!-- Email / Deploy -->
    <section class="card preset-filled-surface-100-900 p-6 space-y-4">
      <h2 class="h3">Email (Deploy)</h2>
      <p class="text-sm opacity-60">
        Sent via the Gmail API using the Google account connected above.
      </p>
      <label class="label">
        <span>From address</span>
        <input class="input" type="email" bind:value={cfg.email.from} />
      </label>
      <label class="label">
        <span>To address (Kindle email)</span>
        <input class="input" type="email" bind:value={cfg.email.to} />
      </label>
    </section>

    <!-- Advanced -->
    <section class="card preset-filled-surface-100-900 p-6 space-y-4">
      <h2 class="h3">Advanced</h2>
      <label class="label">
        <span>Data directory</span>
        <input class="input" type="text" bind:value={cfg.data_dir} />
      </label>
    </section>

    <!-- Save -->
    <div class="flex items-center gap-4">
      <button
        class="btn preset-filled"
        onclick={save}
        disabled={saving || !!data.loadError}
      >
        {saving ? "Saving…" : "Save"}
      </button>
      {#if saved}
        <span class="text-success-500 text-sm">Saved.</span>
      {/if}
    </div>
    {#if error}
      <div
        role="alert"
        class="border-l-4 border-error-500 p-3 text-error-500 whitespace-pre-wrap break-words"
      >
        {error}
      </div>
    {/if}
  </main>
</div>
