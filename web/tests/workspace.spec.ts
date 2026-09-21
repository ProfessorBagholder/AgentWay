import { test, expect } from "@playwright/test";
test("saves an agent and task, updates a second window without reloading or refetching unrelated resources", async ({
  page,
  context,
}) => {
  await page.goto("/");
  await expect(page.locator(".instance")).toHaveText(/Live/);
  const other = await context.newPage();
  await other.goto("/");
  await expect(other.locator(".instance")).toHaveText(/Live/);
  const reads: string[] = [];
  const documents: string[] = [];
  other.on("request", (r) => {
    if (
      r.method() === "GET" &&
      r.url().includes("/api/") &&
      !r.url().includes("/api/events")
    )
      reads.push(r.url());
    if (r.isNavigationRequest()) documents.push(r.url());
  });
  const name = `Coordinator ${Date.now()}`;
  await page
    .getByRole("button", { name: "Register agent", exact: true })
    .click();
  await page.getByLabel("Agent name").fill(name);
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Register agent", exact: true })
    .click();
  await expect(other.getByText(name, { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Tasks", exact: true }).click();
  await page.getByRole("button", { name: "Create task", exact: true }).click();
  const title = `Captions ${Date.now()}`;
  await page.getByLabel("Title", { exact: true }).fill(title);
  await page.getByLabel("Assign to").selectOption({ label: name });
  await page.getByLabel("Instructions").fill("Create three short captions.");
  await page.getByRole("button", { name: "Save assignment" }).click();
  await expect(other.getByText(title, { exact: true })).toBeVisible();
  await other.getByRole("button", { name: "Tasks", exact: true }).click();
  await page
    .getByRole("button", { name: `Cancel ${title}`, exact: true })
    .click();
  await expect(
    other
      .locator(".task-row")
      .filter({ hasText: title })
      .getByText("cancelled", { exact: true }),
  ).toBeVisible();
  expect(reads).toEqual([]);
  expect(documents).toEqual([]);
});
