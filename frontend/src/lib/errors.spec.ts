import { describe, expect, it } from "vitest";
import { ensureOk, errorMessage } from "./errors";

describe("frontend error reporting", () => {
    it("returns successful responses unchanged", async () => {
        const response = new Response(null, { status: 204 });
        expect(await ensureOk(response)).toBe(response);
    });

    it("includes HTTP status and backend text", async () => {
        await expect(
            ensureOk(new Response("pandoc failed", { status: 500 })),
        ).rejects.toThrow("HTTP 500: pandoc failed");
    });

    it("uses JSON error messages", async () => {
        for (const body of [
            { message: "Not connected" },
            { error: "Not connected" },
        ]) {
            await expect(
                ensureOk(Response.json(body, { status: 401 })),
            ).rejects.toThrow("HTTP 401: Not connected");
        }
    });

    it("falls back to status for empty, malformed, and HTML responses", async () => {
        for (const response of [
            new Response("", { status: 502, statusText: "Bad Gateway" }),
            new Response("invalid", {
                status: 502,
                statusText: "Bad Gateway",
                headers: { "content-type": "application/json" },
            }),
            new Response("<html>proxy error</html>", {
                status: 502,
                statusText: "Bad Gateway",
                headers: { "content-type": "text/html" },
            }),
        ]) {
            await expect(ensureOk(response)).rejects.toThrow("HTTP 502: Bad Gateway");
        }
    });

    it("handles network errors and non-Error failures", () => {
        expect(errorMessage(new TypeError("Failed to fetch"))).toBe(
            "Failed to fetch",
        );
        expect(errorMessage("Disconnected")).toBe("Disconnected");
    });
});
