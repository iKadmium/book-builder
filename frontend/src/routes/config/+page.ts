import type { PageLoad } from './$types';
import type { AppConfig } from '$lib/types';

export const ssr = false;

export const load: PageLoad = async ({ fetch }): Promise<AppConfig> => {
    const res = await fetch('/api/config');
    if (!res.ok) {
        return {
            data_dir: 'data',
            forgejo: { url: '', repo: '', pat: '' },
            email: { smtp_host: '', smtp_port: 587, smtp_username: '', smtp_password: '', from: '', to: '' }
        };
    }
    return res.json();
};
