import { test, expect } from "@playwright/test";
const publication = {
  id: "upload",
  title: "Podcast teaser",
  status: "uploaded",
  uploaded_bytes: 4,
  total_bytes: 4,
  video_url: "https://www.youtube.com/watch?v=test",
  error: null,
  created_at: "2026-09-21T12:00:00Z",
  revision: 1,
};
// All records and writes in these tests are intercepted; never mutate the user's account.
test.beforeEach(async ({ page }) => {
  await page.route("**/api/bootstrap", (r) =>
    r.fulfill({ json: { agents: [], tasks: [], cursor: 0 } }),
  );
  await page.route("**/api/publishing/connection", (r) =>
    r.fulfill({
      json: {
        id: "publishing",
        name: "Muse",
        state: "Connected",
        publish_enabled: true,
        activity: null,
      },
    }),
  );
  await page.route("**/api/youtube", (r) =>
    r.fulfill({
      json: {
        configured: true,
        account: { id: "channel", name: "Test channel" },
        private_only: false,
        bridge_url: "https://test.example",
      },
    }),
  );
  await page.route("**/api/publications", (r) =>
    r.fulfill({ json: [publication] }),
  );
  await page.route("**/api/publications/upload", (r) =>
    r.fulfill({
      json: {
        publication,
        settings: {
          privacy: "public",
          made_for_kids: false,
          contains_synthetic_media: true,
          description: "Test description",
        },
        channel_id: "channel",
      },
    }),
  );
  await page.route("**/api/publications/upload/history?*", (r) =>
    r.fulfill({
      json: {
        items: [
          { sequence: 1, publication: { ...publication, status: "queued" } },
          { sequence: 2, publication },
        ],
        next: null,
      },
    }),
  );
  await page.addInitScript(() => {
    class MockEvents extends EventTarget {
      onopen = null;
      onerror = null;
      constructor() {
        super();
        (window as any).testEvents = this;
      }
      close() {}
    }
    (window as any).EventSource = MockEvents;
  });
});
test("real task rows update without navigation or unrelated fetching", async ({
  page,
}) => {
  await page.goto("/#/agents");
  await expect(
    page.getByRole("link", { name: "Muse", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("navigation")
    .getByRole("link", { name: "Tasks", exact: true })
    .click();
  await expect(
    page.getByRole("link", { name: "Podcast teaser", exact: true }),
  ).toBeVisible();
  const reads: string[] = [],
    documents: string[] = [];
  page.on("request", (r) => {
    if (r.url().includes("/api/")) reads.push(r.url());
    if (r.isNavigationRequest()) documents.push(r.url());
  });
  await page.evaluate(
    (p) =>
      (window as any).testEvents.dispatchEvent(
        new MessageEvent("publication.upsert", {
          data: JSON.stringify({
            ...p,
            status: "interrupted",
            error: "Connection interrupted",
            revision: 2,
          }),
        }),
      ),
    publication,
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
test("routes, theme, platform fields and task history survive navigation", async ({
  page,
}) => {
  await page.goto("/#/platforms");
  await expect(
    page.getByRole("columnheader", { name: "Agents", exact: true }),
  ).toBeVisible();
  await page.getByRole("link", { name: "YouTube", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "YouTube", exact: true }),
  ).toBeVisible();
  await page.reload();
  await expect(page.getByText("Test channel", { exact: true })).toBeVisible();
  await page
    .getByRole("navigation")
    .getByRole("link", { name: "Settings", exact: true })
    .click();
  await page.getByRole("radio", { name: "Light", exact: true }).check();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page.reload();
  await expect(
    page.getByRole("radio", { name: "Light", exact: true }),
  ).toBeChecked();
  await page
    .getByRole("navigation")
    .getByRole("link", { name: "Tasks", exact: true })
    .click();
  await page.getByRole("link", { name: "Podcast teaser", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Video settings" }),
  ).toBeVisible();
  await page.getByRole("link", { name: "View activity" }).click();
  await expect(page).toHaveURL(/#\/activity\?task=upload$/);
  await page.locator("summary").click();
  await expect(page.locator(".journal li")).toHaveCount(2);
  await page.goBack();
  await expect(
    page.getByRole("heading", { name: "Video settings" }),
  ).toBeVisible();
  await page.goForward();
  await expect(
    page.getByRole("heading", { name: "Activity log" }),
  ).toBeVisible();
  await page
    .getByRole("navigation")
    .getByRole("link", { name: "Platforms", exact: true })
    .click();
  await page.setViewportSize({ width: 390, height: 844 });
  for (const text of ["YouTube", "Test channel", "Connected", "Muse"])
    await expect(
      page.locator("main").getByText(text, { exact: true }),
    ).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
  ).toBe(false);
});
test("agent permission writes and disconnect require explicit actions", async ({
  page,
}) => {
  let accessWrites = 0,
    disconnectWrites = 0;
  await page.route("**/api/publishing/connection/access", async (r) => {
    accessWrites++;
    expect(r.request().postDataJSON()).toEqual({ publish_enabled: false });
    await r.fulfill({
      json: {
        id: "publishing",
        name: "Muse",
        state: "Connected",
        publish_enabled: false,
        activity: null,
      },
    });
  });
  await page.route("**/api/publishing/connection/disconnect", async (r) => {
    disconnectWrites++;
    await r.fulfill({
      json: {
        id: "publishing",
        name: "Muse",
        state: "Disconnected",
        publish_enabled: false,
        activity: null,
      },
    });
  });
  await page.goto("/#/agents/publishing");
  await page.getByRole("checkbox", { name: "Publish videos" }).uncheck();
  expect(accessWrites).toBe(0);
  await page.getByRole("button", { name: "Save permissions" }).click();
  await expect(
    page.getByRole("button", { name: "Save permissions" }),
  ).toBeDisabled();
  expect(accessWrites).toBe(1);
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();
  expect(disconnectWrites).toBe(0);
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  expect(disconnectWrites).toBe(0);
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();
  await page
    .getByRole("button", { name: "Disconnect agent", exact: true })
    .click();
  await expect(page.getByText("Disconnected", { exact: true })).toBeVisible();
  expect(disconnectWrites).toBe(1);
});
