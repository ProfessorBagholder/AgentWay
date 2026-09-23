import { test, expect } from "@playwright/test";

test("publishing loads real account state and keeps navigation and credentials local", async ({
  page,
}) => {
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
  await page.goto("/#publishing");
  await expect(
    page.getByRole("heading", { name: "YouTube", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("navigation")
    .getByRole("link", { name: "Agents", exact: true })
    .click();
  await page.getByRole("link", { name: "Muse", exact: true }).click();
  await page.getByText("Connection instructions", { exact: true }).click();
  await expect(page.getByLabel("Agent connection instructions")).toHaveValue(
    /POST \/v1\/youtube\/publish/,
  );
  await expect(page.getByLabel("Agent connection instructions")).toHaveValue(
    /agent_guidance/,
  );
  await expect(page.getByLabel("Agent connection instructions")).toHaveValue(
    /made_for_kids/,
  );
  const documents: string[] = [];
  page.on("request", (r) => {
    if (r.isNavigationRequest()) documents.push(r.url());
  });
  await page
    .getByRole("navigation")
    .getByRole("link", { name: "Agents", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Agents", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("navigation")
    .getByRole("link", { name: "Tasks", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Tasks", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("navigation")
    .getByRole("link", { name: "Platforms", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Platforms", exact: true }),
  ).toBeVisible();
  expect(documents).toEqual([]);
  await page.setViewportSize({ width: 390, height: 844 });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
  ).toBe(false);
});

test("publishing shows setup failures without navigating away", async ({
  page,
}) => {
  await page.route("**/api/youtube", (route) =>
    route.fulfill({
      json: {
        configured: false,
        account: null,
        private_only: true,
        bridge_url: "",
      },
    }),
  );
  await page.route("**/api/youtube/config", (route) =>
    route.fulfill({
      status: 400,
      json: { error: "Enter a Google OAuth web client ID and client secret" },
    }),
  );
  await page.goto("/#publishing");
  await page.getByLabel("Client ID", { exact: true }).fill("invalid");
  await page
    .getByLabel("Client secret", { exact: true })
    .fill("dummy-test-value");
  await page.getByRole("button", { name: "Save credentials" }).click();
  await expect(page.getByRole("alert")).toHaveText(
    "Enter a Google OAuth web client ID and client secret",
  );
  await expect(
    page.getByLabel("Client secret", { exact: true }),
  ).toHaveAttribute("type", "password");
});

test("an external authorization page can return to the app", async ({
  page,
  baseURL,
}) => {
  // Reproduce browser Fetch Metadata after consent without using a Google account.
  await page.route("https://oauth-test.example/consent", (route) =>
    route.fulfill({
      contentType: "text/html",
      body: `<a href="${baseURL}/#publishing">Return to AgentWay</a>`,
    }),
  );
  await page.goto("https://oauth-test.example/consent");
  await page.getByRole("link", { name: "Return to AgentWay" }).click();
  await expect(
    page.getByRole("heading", { name: "YouTube", exact: true }),
  ).toBeVisible();
});
