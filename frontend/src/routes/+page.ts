import type { PageLoad } from './$types';
import type { StatusData } from '$lib/types';

// Load runs client-side only; the API isn't available at build time.
export const ssr = false;

export const load: PageLoad = async ({ fetch }): Promise<StatusData> => {
    const res = await fetch('/api/status');
    if (!res.ok) return { lastPull: null, books: {} };

    const raw: Record<string, unknown> = await res.json();
    const { lastPull, ...bookEntries } = raw;

    return {
        lastPull: (lastPull as string | null) ?? null,
        books: bookEntries as StatusData['books']
    };
};
