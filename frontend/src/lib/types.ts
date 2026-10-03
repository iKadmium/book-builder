export interface Chapter {
	path: string;
	wordCount: number;
}

export interface Version {
	chapters: Chapter[];
	wordCount: number;
	lastUpdated: string | null;
	lastBuilt: string | null;
	lastDeployed: string | null;
}

export interface Book extends Version {
	title: string;
	subtitle: string | null;
	blurb: string | null;
	versions: Record<string, Version>;
}

export interface StatusData {
	lastPull: string | null;
	books: Record<string, Book>;
}
export interface ForgejoConfig {
	url: string;
	repo: string;
}

export interface GoogleConfig {
}

export interface EmailConfig {
	from: string;
	to: string;
}

export interface AppConfig {
	data_dir: string;
	forgejo: ForgejoConfig;
	google: GoogleConfig;
	email: EmailConfig;
}
