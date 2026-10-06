<script lang="ts">
  let {
    hasVersion,
    hasBuilt,
    building,
    deploying,
    open = $bindable(false),
    onbuild,
    ondeploy,
    ondownload,
    oncover,
  }: {
    hasVersion: boolean;
    hasBuilt: boolean;
    building: boolean;
    deploying: boolean;
    open?: boolean;
    onbuild: () => void;
    ondeploy: () => void;
    ondownload: (format: "epub" | "md") => void;
    oncover: (format: "svg" | "png" | "jpg") => void;
  } = $props();

  function runAction(action: () => void) {
    open = false;
    action();
  }
</script>

<div class="flex gap-2 pt-2 border-t border-surface-300-700">
  <button
    class="btn preset-tonal"
    onclick={onbuild}
    disabled={!hasVersion || building || deploying}
  >
    {building ? "Building…" : "Build"}
  </button>
  <div class="relative">
    <button
      class="btn preset-tonal"
      onclick={() => (open = !open)}
      disabled={!hasVersion || building || deploying}
    >
      {deploying ? "Deploying…" : "Deploy ▾"}
    </button>
    {#if open}
      <div
        class="fixed inset-0 z-40"
        role="presentation"
        onclick={() => (open = false)}
      ></div>
      <div
        class="card preset-filled-surface-200-800 absolute left-0 mt-1 p-2 shadow-lg min-w-44 z-50 space-y-1"
      >
        <button
          class="btn preset-ghost w-full justify-start text-sm"
          onclick={() => runAction(ondeploy)}
          disabled={!hasBuilt}
        >
          📧 Kindle
        </button>
        <button
          class="btn preset-ghost w-full justify-start text-sm"
          onclick={() => runAction(() => ondownload("epub"))}
          disabled={!hasBuilt}
        >
          ⬇ Download EPUB
        </button>
        <button
          class="btn preset-ghost w-full justify-start text-sm"
          onclick={() => runAction(() => ondownload("md"))}
          disabled={!hasBuilt}
        >
          ⬇ Download MD
        </button>
        <button
          class="btn preset-ghost w-full justify-start text-sm"
          onclick={() => runAction(() => oncover("svg"))}
        >
          Build SVG Cover
        </button>
        <button
          class="btn preset-ghost w-full justify-start text-sm"
          onclick={() => runAction(() => oncover("png"))}
        >
          Build PNG Cover
        </button>
        <button
          class="btn preset-ghost w-full justify-start text-sm"
          onclick={() => runAction(() => oncover("jpg"))}
        >
          Build JPG Cover
        </button>
      </div>
    {/if}
  </div>
</div>
