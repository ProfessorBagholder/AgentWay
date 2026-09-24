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
  await page.route("**/api/media-transfers", (r) =>
    r.fulfill({ json: { items: [], next: null } }),
  );
  await page.route("**/api/agent-handoffs", (r) =>
    r.fulfill({ json: { items: [], next: null } }),
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
test("Grok receiver setup keeps the webhook key private and shows pending verification", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.route("**/api/agent-connections", (route) =>
    route.fulfill({
      json: [
        {
          id: "grok",
          name: "Grok",
          product: "Grok Bot",
          revision: 1,
          state: "Connected",
          publish_enabled: false,
          activity: null,
        },
      ],
    }),
  );
  let configured = false;
  await page.route("**/api/agent-handoffs", (route) =>
    route.fulfill({
      json: {
        items: [
          {
            id: "probe-task",
            sender_name: "Muse",
            recipient_id: "grok",
            delivery_mode: "pull",
            status: "queued",
            title: "AgentWay receiver probe",
            instructions: "Confirm receipt. Do not publish anything.",
            expires_at: Date.now() / 1000 + 1800,
          },
        ],
        next: null,
      },
    }),
  );
  await page.route("**/api/handoff-receivers/grok/probe", (route) => {
    expect(route.request().postDataJSON()).toEqual({ task_id: "probe-task" });
    return route.fulfill({
      json: {
        id: "probe-1",
        task_id: "probe-task",
        transport_state: "admitted",
        task_status: "queued",
      },
    });
  });
  await page.route("**/api/handoff-receivers/grok", async (route) => {
    if (route.request().method() === "POST") {
      const body = route.request().postDataJSON();
      expect(body).toEqual({
        native_target: "https://api2.cursor.sh/automations/webhook/probe",
        grok_webhook_key: "private-key",
      });
      configured = true;
      return route.fulfill({
        json: { enabled: true, ready: false, receiver_token: "private-token" },
      });
    }
    if (route.request().method() === "DELETE") {
      configured = false;
      return route.fulfill({ json: { enabled: false, ready: false } });
    }
    return route.fulfill({
      json: {
        enabled: configured,
        ready: false,
        grok_webhook_configured: configured,
      },
    });
  });
  await page.goto("/#/agents/grok");
  const configure = page.getByRole("link", {
    name: "Configure Grok webhook",
  });
  await expect(configure).toBeInViewport();
  await configure.click();
  await page
    .getByLabel("POST URL")
    .fill("https://api2.cursor.sh/automations/webhook/probe");
  await page.getByLabel("Key", { exact: true }).fill("private-key");
  await page.getByRole("button", { name: "Save webhook" }).click();
  await expect(page.getByText("Saved · Awaiting verification")).toBeVisible();
  await expect(page.getByText("private-key")).toHaveCount(0);
  await expect(page.getByText("private-token")).toHaveCount(0);
  await expect(page.getByLabel("Key", { exact: true })).toHaveValue("");
  await page.getByLabel("Queued task").selectOption("probe-task");
  await page.getByRole("button", { name: "Send one test" }).click();
  await expect(
    page.getByText("Webhook run started. Check the task for an agent result."),
  ).toBeVisible();
  await page.getByRole("button", { name: "Remove receiver" }).click();
  await expect(
    page.getByText("Pending deliveries to this receiver will stop."),
  ).toBeVisible();
  await page.getByRole("button", { name: "Remove receiver" }).click();
  await expect(
    page.getByRole("heading", { name: "Add webhook" }),
  ).toBeVisible();
  await expect(page.getByText("Saved · Awaiting verification")).toHaveCount(0);
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(390);
});
test("agent handoff appears in Tasks and its result updates in place", async ({
  page,
}, testInfo) => {
  const task = {
    cursor: 1,
    id: "handoff-1",
    request_id: "request-1",
    sender_id: "muse",
    sender_name: "Muse",
    recipient_id: "grok",
    recipient_name: "Grok",
    title: "Review episode",
    instructions: "Check the transcript",
    status: "queued",
    delivery_mode: "pull",
    result_acknowledged_at: null,
    result: null,
    error: null,
    lease_until: null,
    timeout_seconds: null,
    expires_at: null,
    created_at: "2026-09-23T12:00:00Z",
    updated_at: "2026-09-23T12:00:00Z",
    revision: 1,
  };
  await page.route("**/api/agent-handoffs", (r) =>
    r.fulfill({ json: { items: [task], next: null } }),
  );
  await page.route("**/api/agent-handoffs/handoff-1", (r) =>
    r.fulfill({ json: task }),
  );
  await page.route("**/api/agent-handoffs/handoff-1/history?*", (r) =>
    r.fulfill({
      json: {
        items: [
          { sequence: 1, task: { ...task, action: "created" } },
          {
            sequence: 2,
            task: {
              ...task,
              action: "completed",
              status: "completed",
              result: "Looks good",
              revision: 2,
            },
          },
        ],
        next: null,
      },
    }),
  );
  const row = page.getByRole("row", { name: /Review episode/ });
  for (const theme of ["dark", "light"]) {
    await page.goto("/#/settings");
    await page
      .getByRole("radio", { name: theme === "dark" ? "Dark" : "Light" })
      .check();
    for (const width of [1440, 900, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto("/#/tasks");
      await expect(row).toContainText("Muse");
      await expect(row).toContainText("Grok");
      await expect(row).toContainText("Awaiting pickup");
      await row.getByRole("link", { name: "Review episode" }).click();
      await expect(
        page.getByText(
          "Waiting for Grok to check AgentWay. No pickup has been recorded.",
        ),
      ).toBeVisible();
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth > innerWidth,
        ),
      ).toBe(false);
      await page.screenshot({
        path: testInfo.outputPath(`${theme}-${width}-awaiting-pickup.png`),
        fullPage: true,
      });
    }
  }
  await page.goto("/#/tasks");
  await page.evaluate(
    (t) =>
      (window as any).testEvents.dispatchEvent(
        new MessageEvent("agent.handoff", {
          data: JSON.stringify({
            ...t,
            status: "completed",
            result: "Looks good",
            revision: 2,
          }),
        }),
      ),
    task,
  );
  await expect(row).toContainText("Completed");
  await row.getByRole("link", { name: "Review episode" }).click();
  await expect(page.getByText("Check the transcript")).toBeVisible();
  await page.getByRole("link", { name: "View activity" }).click();
  await page.getByRole("button", { name: /Review episode/ }).click();
  await expect(page.getByText("Looks good")).toBeVisible();
});
test("timed-out handoff shows a terminal state and its deadline", async ({
  page,
}, testInfo) => {
  const task = {
    cursor: 2,
    id: "timed-out-task",
    request_id: "request-2",
    sender_id: "muse",
    sender_name: "Muse",
    recipient_id: "grok",
    recipient_name: "Grok",
    title: "Review overdue episode",
    instructions: "Check the transcript",
    status: "timed_out",
    result: null,
    error: "Recipient did not claim the task before its deadline.",
    lease_until: null,
    timeout_seconds: 720,
    expires_at: 1790165520,
    created_at: "2026-09-23T12:00:00Z",
    updated_at: "2026-09-23T12:12:00Z",
    revision: 2,
  };
  await page.route("**/api/agent-handoffs", (r) =>
    r.fulfill({ json: { items: [task], next: null } }),
  );
  await page.route("**/api/agent-handoffs/timed-out-task", (r) =>
    r.fulfill({ json: task }),
  );
  for (const theme of ["dark", "light"]) {
    await page.goto("/#/settings");
    await page
      .getByRole("radio", { name: theme === "dark" ? "Dark" : "Light" })
      .check();
    for (const width of [1440, 900, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto("/#/tasks");
      const row = page.getByRole("row", { name: /Review overdue episode/ });
      await expect(row).toContainText("Timed out");
      await row.getByRole("link", { name: "Review overdue episode" }).click();
      await expect(
        page.getByText("Recipient did not claim the task before its deadline."),
      ).toBeVisible();
      await expect(page.getByText("Deadline", { exact: true })).toBeVisible();
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth > innerWidth,
        ),
      ).toBe(false);
      await page.screenshot({
        path: testInfo.outputPath(`${theme}-${width}-timeout.png`),
        fullPage: true,
      });
    }
  }
});
test("task access shows and edits multiple directed agent permissions", async ({
  page,
}) => {
  await page.route("**/api/agent-connections", (r) =>
    r.fulfill({
      json: [
        {
          id: "muse",
          name: "Muse",
          product: "Muse",
          revision: 1,
          state: "Connected",
          publish_enabled: true,
          activity: null,
        },
        {
          id: "grok",
          name: "Grok",
          product: "Grok Bot",
          revision: 1,
          state: "Connected",
          publish_enabled: true,
          activity: null,
        },
        {
          id: "claude",
          name: "Claude",
          product: "Claude",
          revision: 1,
          state: "Connected",
          publish_enabled: false,
          activity: null,
        },
      ],
    }),
  );
  const grants: { sender_id: string; recipient_id: string }[] = [];
  await page.route("**/api/agent-handoff-grants", (route) =>
    route.fulfill({ json: grants }),
  );
  await page.route("**/api/agent-handoff-grants/*", (route) => {
    if (route.request().method() === "POST") {
      const sender_id = route.request().url().split("/").pop()!;
      const body = route.request().postDataJSON() as {
        recipient_id: string;
        enabled: boolean;
      };
      if (body.enabled)
        grants.push({ sender_id, recipient_id: body.recipient_id });
      else {
        const index = grants.findIndex(
          (grant) =>
            grant.sender_id === sender_id &&
            grant.recipient_id === body.recipient_id,
        );
        if (index >= 0) grants.splice(index, 1);
      }
    }
    return route.fulfill({ json: grants.map((g) => g.recipient_id) });
  });
  await page.goto("/#/agents");
  await page.getByRole("link", { name: "Task access" }).click();
  await page
    .getByRole("combobox", { name: "Assigning agent" })
    .selectOption("grok");
  await page
    .getByRole("combobox", { name: "Receiving agent" })
    .selectOption("muse");
  await page.getByRole("button", { name: "Allow task assignment" }).click();
  await expect(
    page.getByRole("row", { name: /Grok Muse Remove/ }),
  ).toBeVisible();
  await page
    .getByRole("combobox", { name: "Assigning agent" })
    .selectOption("claude");
  await page
    .getByRole("combobox", { name: "Receiving agent" })
    .selectOption("muse");
  await page.getByRole("button", { name: "Allow task assignment" }).click();
  await expect(
    page.getByRole("row", { name: /Claude Muse Remove/ }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Remove task access from Grok to Muse" })
    .click();
  await expect(page.getByRole("row", { name: /Grok Muse Remove/ })).toHaveCount(
    0,
  );
  await expect(
    page.getByRole("row", { name: /Claude Muse Remove/ }),
  ).toBeVisible();
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
test("transfer work shows attribution, progress and a single diagnostic chain", async ({
  page,
}) => {
  const transfer = {
    cursor: 1,
    id: "63ed6842-5159-4fa2-a1fa-177aab931fed",
    agent_name: "Grok",
    mime: "video/mp4",
    size: 113246208,
    offset: 33554432,
    status: "interrupted",
    last_error: "media_transport_uncertain",
    created_at: "2026-09-22T12:00:00Z",
    has_publication: false,
  };
  await page.route("**/api/media-transfers", (r) =>
    r.fulfill({ json: { items: [transfer], next: null } }),
  );
  let current = {
    ...transfer,
    last_error: transfer.last_error as string | null,
  };
  await page.route(`**/api/media-transfers/${transfer.id}`, (r) =>
    r.fulfill({ json: current }),
  );
  await page.route(`**/api/media-transfers/${transfer.id}/history?*`, (r) =>
    r.fulfill({
      json: {
        items: [
          { sequence: 1, status: "reserved", offset: 0, size: transfer.size },
          {
            sequence: 2,
            status: "receiving",
            offset: transfer.offset,
            size: transfer.size,
          },
          {
            sequence: 3,
            status: "interrupted",
            offset: transfer.offset,
            size: transfer.size,
            error: transfer.last_error,
          },
        ],
        next: null,
      },
    }),
  );
  await page.goto("/#/tasks");
  const title = "Video transfer · 108 MiB";
  await expect(page.getByRole("link", { name: title })).toBeVisible();
  const row = page.locator(".work-table tr").filter({ hasText: title });
  await expect(row.getByText("Grok", { exact: true })).toBeVisible();
  await expect(row.getByText("media_transport_uncertain")).toBeVisible();
  await page.getByRole("link", { name: title }).click();
  await expect(page.getByText("33,554,432 / 113,246,208 bytes")).toBeVisible();
  await page.getByRole("link", { name: "View activity" }).click();
  await page
    .getByRole("button", { name: /Video transfer.*Transfer media/ })
    .click();
  await expect(page.locator(".journal li")).toHaveCount(3);
  await expect(page.getByText("media_transport_uncertain")).toBeVisible();
  current = {
    ...transfer,
    offset: transfer.size,
    status: "ready",
    last_error: null,
  };
  await page.evaluate((id) => {
    (window as any).testEvents.dispatchEvent(
      new MessageEvent("media.transfer", {
        data: JSON.stringify({ media_id: id }),
      }),
    );
  }, transfer.id);
  await expect(page.getByText("Ready", { exact: true })).toBeVisible();
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(
    page.locator("main").getByText("Grok", { exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
  ).toBe(false);
});
test("a published video's transfer is part of its activity, not another row", async ({
  page,
}) => {
  const id = "63ed6842-5159-4fa2-a1fa-177aab931fed";
  await page.route("**/api/media-transfers", (r) =>
    r.fulfill({
      json: {
        items: [
          {
            cursor: 1,
            id,
            agent_name: "Muse",
            mime: "video/mp4",
            size: 4,
            offset: 4,
            status: "ready",
            last_error: null,
            created_at: "2026-09-21T11:00:00Z",
            has_publication: true,
          },
        ],
        next: null,
      },
    }),
  );
  await page.route("**/api/publications/upload/history?*", (r) =>
    r.fulfill({
      json: {
        media_id: id,
        items: [{ sequence: 2, publication }],
        next: null,
      },
    }),
  );
  await page.route(`**/api/media-transfers/${id}/history?*`, (r) =>
    r.fulfill({
      json: {
        items: [{ sequence: 1, status: "ready", offset: 4, size: 4 }],
        next: null,
      },
    }),
  );
  await page.goto("/#/activity");
  await expect(page.locator(".work-table tbody > tr")).toHaveCount(1);
  await page
    .getByRole("button", { name: /Podcast teaser.*Upload video/ })
    .click();
  await expect(page.locator(".journal li")).toHaveCount(2);
  await expect(page.getByText("Ready", { exact: true })).toBeVisible();
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

test("submitted video settings retain all fields across sizes and themes", async ({
  page,
}, testInfo) => {
  await page.route("**/api/publications/upload", (r) =>
    r.fulfill({
      json: {
        publication,
        channel_id: "channel",
        settings: {
          privacy: "private",
          made_for_kids: false,
          contains_synthetic_media: true,
          description: "Original upload description",
          notify_subscribers: false,
          settings: {
            category_id: "27",
            tags: ["software engineering", "podcast"],
            default_language: "en-CA",
            default_audio_language: "en",
            publish_at: "2099-01-01T18:00:00Z",
            license: "creativeCommon",
            embeddable: false,
            public_stats_viewable: true,
            paid_product_placement: false,
            recording_date: "2026-09-22T00:00:00Z",
            localizations: {
              fr: { title: "Titre", description: "Description française" },
            },
          },
        },
      },
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
    for (const width of [1440, 900, 390]) {
      await page.setViewportSize({ width, height: 1000 });
      await page.goto("/#/tasks/upload");
      await expect(
        page.getByRole("heading", { name: "Submitted video settings" }),
      ).toBeVisible();
      for (const label of [
        "Category",
        "Subscriber notifications",
        "Scheduled publication",
        "Audio language",
        "Paid promotion",
        "Translation (French)",
      ]) {
        await expect(page.getByText(label, { exact: true })).toBeVisible();
      }
      await expect(
        page.getByText("software engineering, podcast", { exact: true }),
      ).toBeVisible();
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth > innerWidth,
        ),
      ).toBe(false);
      await page.screenshot({
        path: testInfo.outputPath(`${theme}-${width}-video-settings.png`),
        fullPage: true,
      });
    }
  }
});
