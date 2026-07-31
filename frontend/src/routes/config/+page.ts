import type { AppConfig } from "$lib/types";
import type { PageLoad } from "./$types";

export const ssr = false;

const empty: AppConfig = {
	data_dir: "data",
	forgejo: { url: "", repo: "" },
	google: {},
	email: { from: "", to: "" },
};

export const load: PageLoad = async ({ fetch }): Promise<AppConfig> => {
	const res = await fetch("/api/config");
	if (!res.ok) return empty;
	return res.json();
};
