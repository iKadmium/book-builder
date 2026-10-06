import { expect, test } from "@playwright/test";
import type { Book, Version } from "../lib/types";

test.use({ baseURL: "http://localhost:4173" });

test("selected version controls manuscript state and every book action", async ({ page }) => {
    const chapters: Version = {
        chapters: [{ path: "01. Current.md", wordCount: 100 }],
        wordCount: 100,
        lastUpdated: "2026-10-01T00:00:00Z",
        lastBuilt: "2026-10-02T00:00:00Z",
        lastDeployed: null,
    };
    const draft: Version = {
        ...chapters,
        chapters: [{ path: "01. Draft.md", wordCount: 200 }],
        wordCount: 200,
        lastBuilt: null,
    };
    const book: Book = {
        ...chapters,
        title: "Display Title",
        subtitle: null,
        blurb: null,
        versions: { Chapters: chapters, "Draft v1": draft },
    };
    const requests: string[] = [];
    await page.route("**/api/**", async (route) => {
        const url = new URL(route.request().url());
        if (url.pathname === "/api/status") {
            await route.fulfill({ json: { lastPull: null, "Folder Name": book } });
            return;
        }
        requests.push(`${url.pathname}:${url.searchParams.get("version")}`);
        if (url.pathname.includes("/cover/")) {
            expect(route.request().method()).toBe("POST");
            const format = url.pathname.endsWith("/svg") ? "svg" : "jpg";
            await route.fulfill({
                headers: {
                    "content-type": format === "svg" ? "image/svg+xml" : "image/jpeg",
                    "content-disposition": `attachment; filename="Display Title (Draft v1) 2026-10-03.${format}"`,
                },
                body: "cover image",
            });
        } else if (url.pathname.startsWith("/api/build/")) {
            draft.lastBuilt = "2026-10-03T00:00:00Z";
            await route.fulfill({ status: 204 });
        } else if (url.pathname.startsWith("/api/download/")) {
            await route.fulfill({
                headers: {
                    "content-type": "text/markdown",
                    "content-disposition": 'attachment; filename="Display Title (Draft v1) 2026-10-03.md"',
                },
                body: "draft manuscript",
            });
        } else {
            await route.fulfill({ status: 204 });
        }
    });
    await page.goto("/");
    const selector = page.getByRole("combobox", { name: "Version for Display Title" });
    await expect(selector).toHaveValue("Chapters");
    await expect(page.getByText("100 words", { exact: true })).toBeVisible();
    await selector.selectOption("Draft v1");
    await expect(page.getByText("200 words", { exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "Deploy", exact: false })).toBeEnabled();
    for (const format of ["svg", "jpg"] as const) {
        await page.getByRole("button", { name: "Deploy", exact: false }).click();
        await expect(page.getByRole("button", { name: "Kindle" })).toBeDisabled();
        await expect(page.getByRole("button", { name: "Download EPUB" })).toBeDisabled();
        await expect(page.getByRole("button", { name: "Download MD" })).toBeDisabled();
        const coverDownloadPromise = page.waitForEvent("download");
        await page.getByRole("button", { name: `Build ${format.toUpperCase()} Cover` }).click();
        const coverDownload = await coverDownloadPromise;
        expect(coverDownload.suggestedFilename()).toBe(`Display Title (Draft v1) 2026-10-03.${format}`);
        expect(requests).toContain(`/api/build/Folder%20Name/cover/${format}:Draft v1`);
        await expect(selector).toBeEnabled();
        expect(draft.lastBuilt).toBeNull();
    }
    await page.getByText("1 chapter", { exact: true }).click();
    await expect(page.getByText("01. Draft.md", { exact: true })).toBeVisible();
    await expect(page.getByText("01. Current.md", { exact: true })).toHaveCount(0);
    await page.getByRole("button", { name: "Build", exact: true }).click();
    await expect(selector).toHaveValue("Draft v1");
    await expect(page.getByRole("button", { name: "Deploy", exact: false })).toBeEnabled();
    await expect.poll(() => requests).toContain("/api/build/Folder%20Name:Draft v1");
    await page.getByRole("button", { name: "Deploy", exact: false }).click();
    await page.getByRole("button", { name: "Kindle" }).click();
    await expect.poll(() => requests).toContain("/api/deploy/kindle/Folder%20Name:Draft v1");
    await expect(selector).toBeEnabled();
    await page.getByRole("button", { name: "Deploy", exact: false }).click();
    const downloadPromise = page.waitForEvent("download");
    await page.getByRole("button", { name: "Download MD" }).click();
    const download = await downloadPromise;
    expect(download.suggestedFilename()).toBe("Display Title (Draft v1) 2026-10-03.md");
    expect(requests).toContain("/api/download/Folder%20Name/md:Draft v1");
});

test("books without versions cannot build or deploy on mobile", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.route("**/api/status", (route) => route.fulfill({
        json: {
            lastPull: null,
            "Empty Book": {
                title: "Empty Book",
                subtitle: null,
                blurb: null,
                versions: {},
            },
        },
    }));
    await page.goto("/");
    await expect(page.getByRole("combobox")).toBeDisabled();
    await expect(page.getByRole("button", { name: "Build", exact: true })).toBeDisabled();
    await expect(page.getByRole("button", { name: "Deploy", exact: false })).toBeDisabled();
});