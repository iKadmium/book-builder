export function errorMessage(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
}

export async function ensureOk(response: Response): Promise<Response> {
    if (response.ok) return response;
    let detail = response.statusText;
    const contentType = response.headers.get("content-type") ?? "";
    if (contentType.includes("application/json")) {
        const body: unknown = await response.json().catch(() => null);
        if (body && typeof body === "object") {
            if ("message" in body && typeof body.message === "string")
                detail = body.message;
            else if ("error" in body && typeof body.error === "string")
                detail = body.error;
        }
    } else if (!contentType.includes("text/html")) {
        detail = (await response.text().catch(() => "")).trim() || detail;
    }
    throw new Error(`HTTP ${response.status}${detail ? `: ${detail}` : ""}`);
}
