import type { AppConfig } from "$lib/types";
import type { PageLoad } from "./$types";
import { ensureOk, errorMessage } from "$lib/errors";

export const ssr = false;

const empty: AppConfig = {
	data_dir: "data",
	forgejo: { url: "", repo: "" },
	google: {},
	email: { from: "", to: "" },
};

export const load: PageLoad = async ({
	fetch,
	url,
}): Promise<{
	config: AppConfig;
	loadError: string | null;
	oauthError: string | null;
}> => {
	const oauthError = url.searchParams.get("oauth_error");
	try {
		const res = await ensureOk(await fetch("/api/config"));
		return { config: await res.json(), loadError: null, oauthError };
	} catch (error) {
		return {
			config: empty,
			loadError: `Failed to load configuration: ${errorMessage(error)}`,
			oauthError,
		};
	}
};
