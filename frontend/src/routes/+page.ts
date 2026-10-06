import type { StatusData } from "$lib/types";
import type { PageLoad } from "./$types";
import { ensureOk, errorMessage } from "$lib/errors";

// Load runs client-side only; the API isn't available at build time.
export const ssr = false;

export const load: PageLoad = async ({
	fetch,
}): Promise<StatusData & { loadError: string | null }> => {
	try {
		const res = await ensureOk(await fetch("/api/status"));
		const raw: Record<string, unknown> = await res.json();
		const { lastPull, ...bookEntries } = raw;
		return {
			lastPull: (lastPull as string | null) ?? null,
			books: bookEntries as StatusData["books"],
			loadError: null,
		};
	} catch (error) {
		return {
			lastPull: null,
			books: {},
			loadError: `Failed to load books: ${errorMessage(error)}`,
		};
	}
};
