export interface Chapter {
    path: string;
    wordCount: number;
}

export interface Book {
    chapters: Chapter[];
    wordCount: number;
    lastUpdated: string | null;
    lastBuilt: string | null;
    lastDeployed: string | null;
}

export interface StatusData {
    lastPull: string | null;
    books: Record<string, Book>;
}
export interface ForgejoConfig {
    url: string;
    repo: string;
    pat: string;
}

export interface EmailConfig {
    smtp_host: string;
    smtp_port: number;
    smtp_username: string;
    smtp_password: string;
    from: string;
    to: string;
}

export interface AppConfig {
    data_dir: string;
    forgejo: ForgejoConfig;
    email: EmailConfig;
}