import assert from "node:assert/strict";
import { assertNoPolicyViolations, recordPolicyViolations } from "./csp-violations.mjs";

/** Exercise native hit-testing, rather than a programmatic annotation click. */
export async function checkIframeComments(browser, origin) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  context.setDefaultTimeout(5000);
  try {
    const page = await context.newPage();
    await recordPolicyViolations(page);
    const reviews = [];
    await page.route("**/app/api/reviews", async (route) => {
      reviews.push(route.request().postDataJSON());
      await route.fulfill({ json: { event_id: "iframe-check", state: "received" } });
    });
    await page.goto(`${origin}/app?case=iframe`, { waitUntil: "networkidle" });
    await page.locator(".cf-topbar").waitFor();
    const frame = page.locator("iframe[title='Sandbox fixture']");
    await page.frameLocator("iframe[title='Sandbox fixture']").getByText("Static sandbox content").waitFor();
    const original = await page.evaluate(() => ({
      frame: document.querySelector("iframe").style.cssText,
      parent: document.querySelector("#frame-figure").getAttribute("style"),
      root: document.querySelector("#cf-present-document").getAttribute("style"),
    }));
    const comment = page.getByRole("button", { name: /Comment/ });
    await comment.click();
    await page.waitForFunction(() => getComputedStyle(document.querySelector("iframe")).pointerEvents === "none");
    await frame.evaluate((el) => el.scrollIntoView({ block: "center", behavior: "instant" }));

    // A transformed, internally clipped figure still resolves to its own
    // anchor, not a viewport-sized synthetic cover or a neighboring block.
    for (const width of [1280, 320]) {
      await page.setViewportSize({ width, height: 900 });
      if (await page.locator(".cf-feedback-close").isVisible()) await page.locator(".cf-feedback-close").click();
      await page.locator("#frame-scroll").evaluate((el) => { el.scrollTop = 80; el.scrollIntoView({ block: "center", behavior: "instant" }); });
      const point = await page.locator("#frame-scroll").evaluate((el) => {
        const r = el.getBoundingClientRect();
        return { x: r.left + r.width / 2, y: r.top + 60 };
      });
      assert.equal(await page.evaluate(({ x, y }) => document.elementFromPoint(x, y)?.id, point), "frame-figure");
      await page.mouse.click(point.x, point.y);
      await page.getByTestId("float-chip").waitFor();
      assert.equal((await page.getByTestId("float-chip").locator(".lab").innerText()).trim(), "Element");
      await page.keyboard.press("Escape");
      await page.getByTestId("float-chip").waitFor({ state: "detached" });
      if (await page.locator(".cf-feedback-close").isVisible()) await page.locator(".cf-feedback-close").click();
    }

    const target = await page.locator("#frame-scroll").evaluate((el) => {
      const r = el.getBoundingClientRect(); return { x: r.left + 20, y: r.top + 50 };
    });
    await page.keyboard.down("Shift");
    await page.mouse.move(target.x, target.y);
    await page.mouse.down();
    await page.mouse.move(target.x + 50, target.y + 50, { steps: 5 });
    await page.mouse.up();
    await page.keyboard.up("Shift");
    await page.getByTestId("float-chip").waitFor();
    assert.equal((await page.getByTestId("float-chip").locator(".lab").innerText()).trim(), "Area");
    await page.keyboard.press("Escape");
    await page.getByTestId("float-chip").waitFor({ state: "detached" });
    if (await page.locator(".cf-feedback-close").isVisible()) await page.locator(".cf-feedback-close").click();
    await page.mouse.click(target.x, target.y);
    await page.getByTestId("float-comment").click();
    await page.getByTestId("composer-text").fill("Review the embedded figure.");
    await page.getByTestId("composer-save").click();
    // At phone width a saved note leaves the notes sheet expanded, with Submit.
    await page.locator("#cf-feedback-panel[data-open='true'][data-expanded='true']").waitFor();
    await page.getByTestId("submit-all").click();
    await page.waitForFunction(() => document.querySelector("#cf-comment-toggle")?.getAttribute("aria-pressed") === "false");
    assert.equal(reviews.length, 1);
    assert.equal(reviews[0].notes[0].block_id, "frame-block");
    assert.equal(reviews[0].notes[0].element_selector.tag_name, "figure");
    assert.equal(reviews[0].notes[0].element_selector.element_path, "div:nth-of-type(1) > figure:nth-of-type(1)");

    // Submission exits Comment and restores iframe interaction and styles.
    await page.waitForFunction(() => getComputedStyle(document.querySelector("iframe")).pointerEvents === "auto");
    assert.deepEqual(await page.evaluate(() => ({
      frame: document.querySelector("iframe").style.cssText,
      parent: document.querySelector("#frame-figure").getAttribute("style"),
      root: document.querySelector("#cf-present-document").getAttribute("style"),
    })), original);
    assert.equal(await page.locator("[data-cf-comment-cover], [data-cf-comment-cover-layer]").count(), 0);
    const point = await page.locator("#frame-scroll").evaluate((el) => {
      const r = el.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + 60 };
    });
    assert.equal(await page.evaluate(({ x, y }) => document.elementFromPoint(x, y)?.tagName, point), "IFRAME");

    // Plain values and absent declarations retain their priority on cleanup.
    await frame.evaluate((el) => el.style.setProperty("pointer-events", "auto"));
    await page.keyboard.press("c");
    await page.waitForFunction(() => getComputedStyle(document.querySelector("iframe")).pointerEvents === "none");
    await page.keyboard.press("c");
    await page.waitForFunction(() => document.querySelector("iframe").style.getPropertyValue("pointer-events") === "auto");
    assert.equal(await frame.evaluate((el) => el.style.getPropertyPriority("pointer-events")), "");
    await frame.evaluate((el) => el.style.removeProperty("pointer-events"));
    for (let i = 0; i < 2; i++) {
      await page.keyboard.press("c");
      await page.waitForFunction(() => getComputedStyle(document.querySelector("iframe")).pointerEvents === "none");
      await page.keyboard.press("c");
      await page.waitForFunction(() => document.querySelector("iframe").style.getPropertyValue("pointer-events") === "");
    }
    await assertNoPolicyViolations(page, "iframe comments");
    process.stdout.write("cf-present iframe comment checks passed: figure targeting, clipping, transforms, reflow, and reversible cleanup\n");
  } finally {
    await context.close();
  }
}
