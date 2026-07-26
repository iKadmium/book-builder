<script lang="ts">
	import { AppBar } from '@skeletonlabs/skeleton-svelte';
</script>

<div class="flex flex-col min-h-screen">
	<AppBar>
		{#snippet lead()}
			<strong class="text-xl">Book Builder</strong>
		{/snippet}
	</AppBar>

	<main class="container mx-auto p-8 space-y-8 flex-1">
		<section class="card preset-filled-surface-100-900 p-8 space-y-4">
			<h2 class="h2">Welcome to Book Builder</h2>
			<p class="opacity-75">Convert your Markdown books to EPUB and send them to your Kindle.</p>
			<div class="flex gap-4">
				<button type="button" class="btn preset-filled-primary-500">Get Started</button>
				<button type="button" class="btn preset-outlined-surface-500">Learn More</button>
			</div>
		</section>

		<div class="grid grid-cols-1 md:grid-cols-3 gap-4">
			<div class="card preset-filled-surface-100-900 p-6 space-y-2">
				<h3 class="h3">Upload Markdown</h3>
				<p class="text-sm opacity-75">Write your book in Markdown format and upload it here.</p>
			</div>
			<div class="card preset-filled-surface-100-900 p-6 space-y-2">
				<h3 class="h3">Compile to EPUB</h3>
				<p class="text-sm opacity-75">Pandoc converts your Markdown to a well-formatted EPUB.</p>
			</div>
			<div class="card preset-filled-surface-100-900 p-6 space-y-2">
				<h3 class="h3">Send to Kindle</h3>
				<p class="text-sm opacity-75">Deliver your book directly to your Kindle email address.</p>
			</div>
		</div>
	</main>
</div>
