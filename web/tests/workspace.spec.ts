import { test, expect } from "@playwright/test";

// Browser tests use fixtures and synthetic SSE, never create records in the user's database.
test.beforeEach(async ({ page }) => {
  await page.route("**/api/bootstrap", (route) =>
    route.fulfill({
      json: {
        agents: [{ id: "old", name: "Old placeholder" }],
        tasks: [],
        cursor: 0,
      },
    }),
  );
  await page.route("**/api/publishing/connection", (route) =>
    route.fulfill({ json: { name: "Muse", activity: null } }),
  );
  await page.route("**/api/publications", (route) =>
    route.fulfill({
      json: [
        {
          id: "upload",
          title: "Podcast teaser",
          status: "uploaded",
          uploaded_bytes: 4,
          total_bytes: 4,
          video_url: "https://www.youtube.com/watch?v=test",
          error: null,
          created_at: "2026-09-21T12:00:00Z",
          revision: 1,
        },
      ],
    }),
  );
  await page.addInitScript(() => {
    class MockEvents extends EventTarget {
      onopen: (() => void) | null = null;
      onerror: (() => void) | null = null;
      constructor() {
        super();
        (window as any).testEvents = this;
      }
      close() {}
    }
    (window as any).EventSource = MockEvents;
  });
});

test("shows real publication records and updates individual rows from events", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByText("Muse", { exact: true })).toBeVisible();
  await expect(page.getByText("Old placeholder")).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Add agent", exact: true }),
  ).toHaveCount(0);
  await page.evaluate(() =>
    (window as any).testEvents.dispatchEvent(
      new MessageEvent("bridge.activity", {
        data: JSON.stringify({
          last_seen: "2026-09-21T12:00:00Z",
          operation: "Requested YouTube upload",
          revision: 1,
        }),
      }),
    ),
  );
  await expect(
    page.getByText("Requested YouTube upload", { exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Tasks", exact: true }).click();
  await expect(page.getByText("Podcast teaser", { exact: true })).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Save task", exact: true }),
  ).toHaveCount(0);
  const reads: string[] = [];
  const documents: string[] = [];
  page.on("request", (r) => {
    if (r.url().includes("/api/")) reads.push(r.url());
    if (r.isNavigationRequest()) documents.push(r.url());
  });
  await page.evaluate(() =>
    (window as any).testEvents.dispatchEvent(
      new MessageEvent("publication.upsert", {
        data: JSON.stringify({
          id: "upload",
          title: "Podcast teaser",
          status: "interrupted",
          uploaded_bytes: 2,
          total_bytes: 4,
          video_url: null,
          error: "Connection interrupted",
          created_at: "2026-09-21T12:00:00Z",
          revision: 2,
        }),
      }),
    ),
  );
  await expect(
    page.getByText("Connection interrupted", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Retry upload" }),
  ).toBeVisible();
  expect(reads).toEqual([]);
  expect(documents).toEqual([]);
});

test("tab URLs survive refresh and browser history", async ({ page }) => {
  await page.goto("/#agents");
  await page.getByRole("button", { name: "Tasks", exact: true }).click();
  await expect(page).toHaveURL(/#tasks$/);
  await page.reload();
  await expect(
    page.getByRole("heading", { name: "Tasks", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Publishing", exact: true }).click();
  await expect(page).toHaveURL(/#publishing$/);
  await page.goBack();
  await expect(
    page.getByRole("heading", { name: "Tasks", exact: true }),
  ).toBeVisible();
  await page.goForward();
  await expect(
    page.getByRole("heading", { name: "Publishing", exact: true }),
  ).toBeVisible();
  await page.getByRole("link", { name: "AgentWay", exact: true }).click();
  await expect(page).toHaveURL(/#agents$/);
  await page.reload();
  await expect(
    page.getByRole("heading", { name: "Agents", exact: true }),
  ).toBeVisible();
});
