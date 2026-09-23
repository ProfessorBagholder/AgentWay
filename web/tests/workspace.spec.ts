import { test, expect } from "@playwright/test";
const publication = {
  id: "upload",
  agent_name: "Muse",
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
  await page.route("**/api/agent-connections", (r) =>
    r.fulfill({
      json: [
        {
          id: "publishing",
          name: "Muse",
          product: "Muse",
          revision: 1,
          state: "Connected",
          publish_enabled: true,
          activity: null,
        },
      ],
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
  await page
    .getByRole("button", { name: /Podcast teaser.*Upload video/ })
    .click();
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
  await page.route("**/api/agent-connections/publishing/access", async (r) => {
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
  await page.route(
    "**/api/agent-connections/publishing/disconnect",
    async (r) => {
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
    },
  );
  await page.goto("/#/agents/publishing");
  await page
    .getByRole("checkbox", { name: "Publish and manage YouTube" })
    .uncheck();
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

test("settings align controls and save only edited values without reloading", async ({
  page,
}) => {
  let writes = 0;
  await page.route("**/api/publishing/bridge", async (r) => {
    writes++;
    await r.fulfill({
      json: {
        configured: true,
        account: { id: "channel", name: "Test channel" },
        private_only: false,
        bridge_url: r.request().postDataJSON().url,
      },
    });
  });
  await page.goto("/#/settings");
  const save = page.getByRole("button", { name: "Save", exact: true });
  await expect(save).toBeDisabled();
  const documents: string[] = [];
  page.on("request", (r) => {
    if (r.isNavigationRequest()) documents.push(r.url());
  });
  await page
    .getByRole("textbox", { name: "Agent endpoint" })
    .fill("https://updated.example");
  await expect(save).toBeEnabled();
  await save.click();
  await expect(page.getByRole("status")).toHaveText("Saved");
  await expect(save).toBeDisabled();
  expect(writes).toBe(1);
  await page
    .getByRole("textbox", { name: "Agent endpoint" })
    .fill("https://another.example");
  await expect(page.getByText("Saved", { exact: true })).toHaveCount(0);
  await page.getByRole("radio", { name: "Light", exact: true }).check();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  expect(documents).toEqual([]);
});
test("operational lists retain attribution and aligned columns at all sizes", async ({
  page,
}) => {
  for (const width of [1440, 900, 390]) {
    await page.setViewportSize({ width, height: 900 });
    for (const path of ["tasks", "activity"]) {
      await page.goto(`/#/${path}`);
      const row = page.locator(".work-table tbody tr").first();
      for (const text of ["Muse", "YouTube", "Uploaded"])
        await expect(row.getByText(text, { exact: true })).toBeVisible();
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth > innerWidth,
        ),
      ).toBe(false);
    }
  }
});

test("connect a second agent and update only its authenticated status", async ({
  page,
}) => {
  const grok = {
    id: "grok-test",
    name: "Grok",
    product: "Grok Bot",
    revision: 1,
    state: "Setup incomplete",
    publish_enabled: true,
    activity: null,
  };
  let creates = 0;
  await page.route("**/api/agent-connections", async (r) => {
    if (r.request().method() === "POST") {
      creates++;
      expect(r.request().postDataJSON()).toEqual({
        product: "Grok Bot",
        publish_enabled: true,
      });
      if (creates === 1) {
        await r.fulfill({
          status: 503,
          json: { error: "Connection could not be created. Try again." },
        });
        return;
      }
      await r.fulfill({ json: grok });
    } else
      await r.fulfill({
        json: [
          {
            id: "publishing",
            name: "Muse",
            product: "Muse",
            revision: 1,
            state: "Connected",
            publish_enabled: true,
            activity: null,
          },
        ],
      });
  });
  await page.goto("/#/agents");
  await page.getByRole("link", { name: "Connect agent", exact: true }).click();
  await page
    .getByRole("checkbox", { name: "Publish and manage YouTube" })
    .check();
  await page.getByRole("button", { name: "Continue", exact: true }).click();
  await expect(page.getByRole("alert")).toHaveText(
    "Connection could not be created. Try again.",
  );
  await expect(
    page.getByRole("checkbox", { name: "Publish and manage YouTube" }),
  ).toBeChecked();
  await page.getByRole("button", { name: "Continue", exact: true }).click();
  await expect(page).toHaveURL(/#\/agents\/grok-test$/);
  await expect(
    page.getByRole("heading", { name: "Grok", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("Setup incomplete", { exact: true }),
  ).toBeVisible();
  await expect(page.getByLabel("Agent connection instructions")).toHaveValue(
    /https:\/\/test.example\/mcp/,
  );
  const reads: string[] = [],
    documents: string[] = [];
  page.on("request", (r) => {
    if (r.url().includes("/api/")) reads.push(r.url());
    if (r.isNavigationRequest()) documents.push(r.url());
  });
  await page.evaluate(
    (c) =>
      (window as any).testEvents.dispatchEvent(
        new MessageEvent("agent.connection", {
          data: JSON.stringify({ ...c, state: "Connected", revision: 2 }),
        }),
      ),
    grok,
  );
  await expect(page.getByText("Connected", { exact: true })).toBeVisible();
  await page
    .getByRole("navigation")
    .getByRole("link", { name: "Agents", exact: true })
    .click();
  await expect(
    page.getByRole("link", { name: "Muse", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("link", { name: "Grok", exact: true }),
  ).toBeVisible();
  expect(creates).toBe(2);
  expect(reads).toEqual([]);
  expect(documents).toEqual([]);
});

test("connection screens retain alignment and controls across sizes and themes", async ({
  page,
}, testInfo) => {
  await page.route("**/api/agent-connections", (r) =>
    r.fulfill({
      json: [
        {
          id: "publishing",
          name: "Muse",
          product: "Muse",
          revision: 1,
          state: "Connected",
          publish_enabled: true,
          activity: null,
        },
        {
          id: "grok-test",
          name: "Grok",
          product: "Grok Bot",
          revision: 1,
          state: "Setup incomplete",
          publish_enabled: true,
          activity: null,
        },
      ],
    }),
  );
  for (const theme of ["dark", "light"]) {
    await page.goto("/#/settings");
    await page
      .getByRole("radio", {
        name: theme === "dark" ? "Dark" : "Light",
        exact: true,
      })
      .check();
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    for (const width of [1440, 900, 390]) {
      await page.setViewportSize({ width, height: 1000 });
      for (const route of [
        "agents",
        "agents/connect",
        "agents/grok-test",
        "platforms",
      ]) {
        await page.goto(`/#/${route}`);
        await expect(page.locator("main h1")).toBeVisible();
        if (route === "agents")
          await expect(
            page.getByRole("link", { name: "Grok", exact: true }),
          ).toBeVisible();
        if (route === "agents/connect") {
          const select = page.getByLabel("Agent", { exact: true });
          await expect(select).toBeVisible();
          expect((await select.boundingBox())!.height).toBeGreaterThanOrEqual(
            44,
          );
          await select.focus();
        }
        if (route === "agents/grok-test")
          await expect(
            page.getByLabel("Agent connection instructions"),
          ).toBeVisible();
        expect(
          await page.evaluate(
            () => document.documentElement.scrollWidth > innerWidth,
          ),
        ).toBe(false);
        await page.screenshot({
          path: testInfo.outputPath(
            `${theme}-${width}-${route.replaceAll("/", "-")}.png`,
          ),
          fullPage: true,
        });
      }
    }
  }
});
