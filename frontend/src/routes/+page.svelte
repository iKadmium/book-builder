<script lang="ts">
import { AppBar } from "@skeletonlabs/skeleton-svelte";
import { invalidateAll } from "$app/navigation";
import type { PageData } from "./$types";

let { data }: { data: PageData } = $props();

let pulling = $state(false);
let building = $state<Record<string, boolean>>({});

function fmt(iso: string | null): string {
	if (!iso) return "Never";
	return new Intl.DateTimeFormat(undefined, {
		dateStyle: "medium",
		timeStyle: "short",
	}).format(new Date(iso));
}

function isUpToDate(stamp: string | null, lastUpdated: string | null): boolean {
	if (!stamp) return false;
	if (!lastUpdated) return true;
	return new Date(stamp) >= new Date(lastUpdated);
}

async function pull() {
	pulling = true;
	try {
		await fetch("/api/pull", { method: "POST" });
		await invalidateAll();
	} finally {
		pulling = false;
	}
}

async function buildBook(title: string) {
	building[title] = true;
	try {
		const res = await fetch(`/api/build/${encodeURIComponent(title)}`, {
			method: "POST",
		});
		if (!res.ok) {
			const text = await res.text();
			alert(`Build failed: ${text}`);
		} else {
			await invalidateAll();
		}
	} finally {
		building[title] = false;
	}
}

let deploying = $state<Record<string, boolean>>({});

async function deployBook(title: string) {
	deploying[title] = true;
	try {
		const res = await fetch(`/api/deploy/${encodeURIComponent(title)}`, {
			method: "POST",
		});
		if (!res.ok) {
			const text = await res.text();
			alert(`Deploy failed: ${text}`);
		} else {
			await invalidateAll();
		}
	} finally {
		deploying[title] = false;
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
    {#if Object.keys(data.books).length === 0}
      <p class="opacity-60">No books found. Try pulling the latest changes.</p>
    {:else}
      <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {#each Object.entries(data.books).sort( ([a], [b]) => a.localeCompare(b), ) as [title, book]}
          {@const builtOk = isUpToDate(book.lastBuilt, book.lastUpdated)}
          {@const deployedOk = isUpToDate(book.lastDeployed, book.lastUpdated)}
          <div class="card preset-filled-surface-100-900 p-6 space-y-4">
            <!-- Header -->
            <div class="flex items-start justify-between gap-4">
              <div>
                <h2 class="h3">{title}</h2>
                <p class="text-sm opacity-60">
                  {book.wordCount.toLocaleString()} words
                </p>
              </div>
              <div class="flex gap-2 shrink-0">
                <button
                  class="btn preset-tonal"
                  onclick={() => buildBook(title)}
                  disabled={building[title]}
                >
                  {building[title] ? "Building…" : "Build"}
                </button>
                <button
                  class="btn preset-tonal"
                  onclick={() => deployBook(title)}
                  disabled={deploying[title]}
                >
                  {deploying[title] ? "Deploying…" : "Deploy"}
                </button>
              </div>
            </div>

            <!-- Timestamps -->
            <div class="grid grid-cols-3 gap-2 text-sm">
              <div>
                <p class="opacity-50 text-xs uppercase tracking-wide">
                  Updated
                </p>
                <p>{fmt(book.lastUpdated)}</p>
              </div>
              <div>
                <p class="opacity-50 text-xs uppercase tracking-wide">Built</p>
                <p
                  class:text-success-500={builtOk}
                  class:text-warning-500={!builtOk}
                >
                  {fmt(book.lastBuilt)}
                </p>
              </div>
              <div>
                <p class="opacity-50 text-xs uppercase tracking-wide">
                  Deployed
                </p>
                <p
                  class:text-success-500={deployedOk}
                  class:text-warning-500={!deployedOk}
                >
                  {fmt(book.lastDeployed)}
                </p>
              </div>
            </div>

            <!-- Chapter list -->
            <details>
              <summary class="cursor-pointer text-sm opacity-60 select-none">
                {book.chapters.length} chapter{book.chapters.length !== 1
                  ? "s"
                  : ""}
              </summary>
              <ol class="mt-2 space-y-1">
                {#each book.chapters as chapter}
                  <li class="flex justify-between text-sm">
                    <span class="opacity-80">{chapter.path}</span>
                    <span class="opacity-50 tabular-nums"
                      >{chapter.wordCount.toLocaleString()} w</span
                    >
                  </li>
                {/each}
              </ol>
            </details>
          </div>
        {/each}
      </div>
    {/if}
  </main>
</div>
