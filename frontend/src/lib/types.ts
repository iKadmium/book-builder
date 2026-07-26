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
export interface OAuth2Credentials {
    client_id: string;
    client_secret: string;
}

export interface ForgejoConfig {
    url: string;
    repo: string;
    oauth: OAuth2Credentials;
}

export interface GoogleConfig {
    oauth: OAuth2Credentials;
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