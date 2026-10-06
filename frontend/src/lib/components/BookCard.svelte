<script lang="ts">
  import { invalidateAll } from "$app/navigation";
  import { marked } from "marked";
  import DOMPurify from "dompurify";
  import { ensureOk, errorMessage } from "$lib/errors";
  import type { Book } from "$lib/types";
  import BookActions from "./BookActions.svelte";

  let { folderName, book }: { folderName: string; book: Book } = $props();

  let selectedVersion = $state("");
  let building = $state(false);
  let deploying = $state(false);
  let error = $state("");
  let deployMenuOpen = $state(false);

  const name = $derived.by(() => {
    if (selectedVersion && book.versions[selectedVersion])
      return selectedVersion;
    return book.versions.Chapters
      ? "Chapters"
      : (Object.keys(book.versions).sort()[0] ?? "");
  });
  const version = $derived(book.versions[name]);
  const builtOk = $derived(
    isUpToDate(version?.lastBuilt ?? null, version?.lastUpdated ?? null),
  );
  const deployedOk = $derived(
    isUpToDate(version?.lastDeployed ?? null, version?.lastUpdated ?? null),
  );

  function versionQuery(version: string): string {
    return `?${new URLSearchParams({ version })}`;
  }

  function renderBlurb(markdown: string): string {
    return DOMPurify.sanitize(marked.parse(markdown, { async: false }));
  }

  function fmt(iso: string | null): string {
    if (!iso) return "Never";
    return new Intl.DateTimeFormat(undefined, {
      dateStyle: "medium",
      timeStyle: "short",
    }).format(new Date(iso));
  }

  function isUpToDate(
    stamp: string | null,
    lastUpdated: string | null,
  ): boolean {
    if (!stamp) return false;
    if (!lastUpdated) return true;
    return new Date(stamp) >= new Date(lastUpdated);
  }

  async function buildBook(version: string) {
    building = true;
    error = "";
    try {
      const res = await fetch(
        `/api/build/${encodeURIComponent(folderName)}${versionQuery(version)}`,
        { method: "POST" },
      );
      await ensureOk(res);
      await invalidateAll();
    } catch (cause) {
      error = `Build failed: ${errorMessage(cause)}`;
    } finally {
      building = false;
    }
  }

  async function buildCover(version: string, format: "svg" | "jpg") {
    building = true;
    error = "";
    try {
      const res = await fetch(
        `/api/build/${encodeURIComponent(folderName)}/cover/${format}${versionQuery(version)}`,
        { method: "POST" },
      );
      await ensureOk(res);
      const url = URL.createObjectURL(await res.blob());
      try {
        const link = document.createElement("a");
        link.href = url;
        link.download =
          res.headers
            .get("content-disposition")
            ?.match(/filename="([^"]+)"/)?.[1] ??
          `${book.title} (${version}).${format}`;
        document.body.appendChild(link);
        link.click();
        link.remove();
      } finally {
        setTimeout(() => URL.revokeObjectURL(url), 1000);
      }
    } catch (cause) {
      error = `Cover build failed: ${errorMessage(cause)}`;
    } finally {
      building = false;
    }
  }

  async function deployKindle(version: string) {
    deploying = true;
    error = "";
    try {
      const res = await fetch(
        `/api/deploy/kindle/${encodeURIComponent(folderName)}${versionQuery(version)}`,
        { method: "POST" },
      );
      await ensureOk(res);
      await invalidateAll();
    } catch (cause) {
      error = `Kindle deploy failed: ${errorMessage(cause)}`;
    } finally {
      deploying = false;
    }
  }

  async function downloadFile(version: string, format: "epub" | "md") {
    building = true;
    error = "";
    try {
      const response = await ensureOk(
        await fetch(
          `/api/download/${encodeURIComponent(folderName)}/${format}${versionQuery(version)}`,
        ),
      );
      const url = URL.createObjectURL(await response.blob());
      try {
        const link = document.createElement("a");
        link.href = url;
        link.download =
          response.headers
            .get("content-disposition")
            ?.match(/filename="([^"]+)"/)?.[1] ??
          `${book.title} (${version}).${format}`;
        document.body.appendChild(link);
        link.click();
        link.remove();
      } finally {
        setTimeout(() => URL.revokeObjectURL(url), 1000);
      }
    } catch (cause) {
      error = `Download failed: ${errorMessage(cause)}`;
    } finally {
      building = false;
    }
  }
</script>

<div class="card preset-filled-surface-100-900 p-6 space-y-4">
  {#if error}
    <div
      role="alert"
      class="border-l-4 border-error-500 p-3 text-error-500 whitespace-pre-wrap break-words max-h-64 overflow-auto"
    >
      {error}
    </div>
  {/if}
  <div>
    <h2 class="h3">{book.title}</h2>
    {#if book.subtitle}
      <p class="text-sm opacity-70 italic">{book.subtitle}</p>
    {/if}
    <p class="text-sm opacity-60">
      {(version?.wordCount ?? 0).toLocaleString()} words
    </p>
  </div>

  <label class="flex items-center gap-3 text-sm">
    <span>Version</span>
    <select
      class="select min-w-0 flex-1 pl-2"
      aria-label={`Version for ${book.title}`}
      value={name}
      disabled={!version || building || deploying}
      onchange={(event) => {
        selectedVersion = event.currentTarget.value;
        deployMenuOpen = false;
      }}
    >
      {#if !version}
        <option value="">No versions</option>
      {/if}
      {#each Object.keys(book.versions).sort() as versionName}
        <option value={versionName}>{versionName}</option>
      {/each}
    </select>
  </label>

  {#if book.blurb}
    <details>
      <summary class="cursor-pointer text-sm opacity-60 select-none"
        >Blurb</summary
      >
      <div class="mt-2 text-sm space-y-2">
        {@html renderBlurb(book.blurb)}
      </div>
    </details>
  {/if}

  <div class="grid grid-cols-3 gap-2 text-sm">
    <div>
      <p class="opacity-50 text-xs uppercase tracking-wide">Updated</p>
      <p>{fmt(version?.lastUpdated ?? null)}</p>
    </div>
    <div>
      <p class="opacity-50 text-xs uppercase tracking-wide">Built</p>
      <p class:text-success-500={builtOk} class:text-warning-500={!builtOk}>
        {fmt(version?.lastBuilt ?? null)}
      </p>
    </div>
    <div>
      <p class="opacity-50 text-xs uppercase tracking-wide">Deployed</p>
      <p
        class:text-success-500={deployedOk}
        class:text-warning-500={!deployedOk}
      >
        {fmt(version?.lastDeployed ?? null)}
      </p>
    </div>
  </div>

  <details>
    <summary class="cursor-pointer text-sm opacity-60 select-none">
      {version?.chapters.length ?? 0} chapter{version?.chapters.length !== 1
        ? "s"
        : ""}
    </summary>
    <ol class="mt-2 space-y-1">
      {#each version?.chapters ?? [] as chapter}
        <li class="flex justify-between text-sm">
          <span class="opacity-80">{chapter.path}</span>
          <span class="opacity-50 tabular-nums"
            >{chapter.wordCount.toLocaleString()} w</span
          >
        </li>
      {/each}
    </ol>
  </details>

  <BookActions
    hasVersion={!!version}
    hasBuilt={!!version?.lastBuilt}
    {building}
    {deploying}
    bind:open={deployMenuOpen}
    onbuild={() => buildBook(name)}
    ondeploy={() => deployKindle(name)}
    ondownload={(format) => downloadFile(name, format)}
    oncover={(format) => buildCover(name, format)}
  />
</div>
