import type { PageLoad } from './$types';
import type { AppConfig } from '$lib/types';

export const ssr = false;

const empty: AppConfig = {
    data_dir: 'data',
    forgejo: { url: '', repo: '', oauth: { client_id: '', client_secret: '' } },
    google: { oauth: { client_id: '', client_secret: '' } },
    email: { from: '', to: '' },
};

export const load: PageLoad = async ({ fetch }): Promise<AppConfig> => {
    const res = await fetch('/api/config');
    if (!res.ok) return empty;
    return res.json();
};
