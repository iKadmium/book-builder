<script lang="ts">
  import { AppBar } from "@skeletonlabs/skeleton-svelte";
  import { invalidateAll } from "$app/navigation";
  import { ensureOk, errorMessage } from "$lib/errors";
  import BookCard from "$lib/components/BookCard.svelte";
  import type { PageData } from "./$types";

  let { data }: { data: PageData } = $props();

  let pulling = $state(false);
  let pullError = $state("");

  function fmt(iso: string | null): string {
    if (!iso) return "Never";
    return new Intl.DateTimeFormat(undefined, {
      dateStyle: "medium",
      timeStyle: "short",
    }).format(new Date(iso));
  }

  async function pull() {
    pulling = true;
    pullError = "";
    try {
      await ensureOk(await fetch("/api/pull", { method: "POST" }));
      await invalidateAll();
    } catch (error) {
      pullError = `Pull failed: ${errorMessage(error)}`;
    } finally {
      pulling = false;
    }
  }
</script>

<div class="flex flex-col min-h-screen">
  <AppBar>
    <AppBar.Toolbar>
      <AppBar.Lead>
        <strong class="text-xl">Book Builder</strong>
      </AppBar.Lead>
      <AppBar.Trail>
        <div class="flex items-center gap-4">
          <span class="text-sm opacity-60">
            Last pull: {fmt(data.lastPull)}
          </span>
          <button class="btn preset-filled" onclick={pull} disabled={pulling}>
            {pulling ? "Pulling…" : "Pull"}
          </button>
          <a href="/config" class="btn preset-tonal">⚙ Config</a>
        </div>
      </AppBar.Trail>
    </AppBar.Toolbar>
  </AppBar>

  <main class="container mx-auto p-8">
    {#if data.loadError}
      <div
        role="alert"
        class="mb-4 border-l-4 border-error-500 p-3 text-error-500 whitespace-pre-wrap break-words"
      >
        {data.loadError}
      </div>
    {/if}
    {#if pullError}
      <div
        role="alert"
        class="mb-4 border-l-4 border-error-500 p-3 text-error-500 whitespace-pre-wrap break-words"
      >
        {pullError}
      </div>
    {/if}
    {#if Object.keys(data.books).length === 0}
      {#if !data.loadError}
        <p class="opacity-60">
          No books found. Try pulling the latest changes.
        </p>
      {/if}
    {:else}
      <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {#each Object.entries(data.books).sort( ([a], [b]) => a.localeCompare(b), ) as [folderName, book] (folderName)}
          <BookCard {folderName} {book} />
        {/each}
      </div>
    {/if}
  </main>
</div>
