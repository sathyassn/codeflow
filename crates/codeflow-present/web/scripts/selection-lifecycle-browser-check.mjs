import assert from "node:assert/strict";
import { assertNoPolicyViolations, recordPolicyViolations } from "./csp-violations.mjs";

/** Verify the real chrome and submitted anchors, not a replica of its timer. */
export async function checkSelectionLifecycle(browser, origin) {
  const context = await browser.newContext();
  const errors = [];
  const posts = [];
  try {
    const page = await context.newPage();
    await recordPolicyViolations(page);
    page.on("pageerror", (error) => errors.push(error.message));
    await page.route("**/app/api/reviews", async (route) => {
      posts.push(route.request().postDataJSON());
      await route.fulfill({ status: 200, contentType: "application/json", body: '{"event_id":"selection-proof","state":"received"}' });
    });
    await page.goto(`${origin}/app?case=selection`, { waitUntil: "networkidle" });
    const chip = page.getByTestId("float-chip");
    async function arm() {
      if ((await page.locator(".cf-chrome-frame").getAttribute("data-commenting")) !== "true") {
        await page.getByTestId("comment-btn").click();
      }
      // This is the effect-owned readiness marker, not just the button render:
      // programmatic selection must not race Preact's listener installation.
      await page.waitForFunction(() => document.querySelector("#cf-present-document")?.getAttribute("data-cf-commenting") === "true");
    }
    async function select(id, start, end) {
      await page.evaluate(({ id, start, end }) => {
        const text = document.getElementById(id).firstChild;
        const range = document.createRange();
        range.setStart(text, start);
        range.setEnd(text, end);
        getSelection().removeAllRanges();
        getSelection().addRange(range);
        document.dispatchEvent(new Event("selectionchange"));
      }, { id, start, end });
    }
    async function expectQuote(quote) {
      await chip.waitFor({ state: "visible" });
      await page.waitForFunction((expected) => document.querySelector('[data-testid="float-chip"] .q')?.textContent === expected, quote);
    }
    async function dismiss() {
      await page.keyboard.press("Escape");
      await chip.waitFor({ state: "detached" });
    }
    async function saveAndSubmit() {
      await page.getByTestId("composer-text").fill("Anchor proof");
      await page.getByTestId("composer-save").click();
      await page.getByTestId("composer").waitFor({ state: "detached" });
      posts.length = 0;
      await page.getByTestId("submit-all").click();
      await page.getByTestId("toast").getByText(/Review received/).waitFor();
      assert.equal(posts.length, 1);
      assert.equal(posts[0].notes.length, 1);
      return posts[0].notes[0];
    }
    await arm();

    for (const invalid of ["cleared", "collapsed", "cross-root", "oversized"]) {
      await page.evaluate((kind) => {
        const text = document.getElementById("selection-limit").firstChild;
        const selection = getSelection();
        function apply(range) {
          selection.removeAllRanges();
          selection.addRange(range);
          // Deliver the intermediate valid event even when the browser would
          // coalesce it with the later event: this must actually arm the timer.
          document.dispatchEvent(new Event("selectionchange"));
        }
        const valid = document.createRange();
        valid.setStart(text, 0);
        valid.setEnd(text, 6);
        apply(valid);
        const changed = valid.cloneRange();
        if (kind === "cleared") selection.removeAllRanges();
        else if (kind === "collapsed") selection.collapse(text, 0);
        else {
          if (kind === "oversized") changed.setEnd(text, 17);
          else changed.setEnd(document.getElementById("selection-repeat").firstChild, 6);
          apply(changed);
        }
        document.dispatchEvent(new Event("selectionchange"));
      }, invalid);
      // Absence must hold beyond the production 160 ms debounce, not just in
      // the instant after selectionchange. This is not a retry or a fix timeout.
      await page.waitForTimeout(240);
      assert.equal(await chip.count(), 0, `${invalid} retained a stale capture`);
      assert.deepEqual(errors, [], `${invalid} raised a browser error`);
      await select("selection-limit", 0, 6);
      await expectQuote("012345");
      await dismiss();
    }

    for (const length of [15, 16, 17]) {
      await select("selection-limit", 0, length);
      if (length <= 16) {
        await expectQuote("0123456789abcdef".slice(0, length));
        await dismiss();
      } else {
        await page.getByText("Selected text is too long. Select at most 16 characters.").waitFor();
        await page.waitForTimeout(240);
        assert.equal(await chip.count(), 0, "Over-limit selection was pinned");
      }
    }

    await select("selection-limit", 0, 6);
    await select("selection-limit", 10, 16);
    await expectQuote("abcdef");
    await dismiss();
    await select("selection-limit", 10, 16);
    await expectQuote("abcdef"); // Same anchor can be pinned again after Escape.
    await dismiss();

    await select("selection-limit", 0, 6);
    await expectQuote("012345");
    await page.keyboard.press("c");
    await page.waitForFunction(() => document.querySelector(".cf-chrome-frame")?.getAttribute("data-commenting") === "false");
    await chip.waitFor({ state: "detached" });
    await arm();
    await select("selection-limit", 0, 6);
    await chip.waitFor({ state: "visible", timeout: 5000 });
    await expectQuote("012345"); // Exiting Comment releases dedup ownership too.
    await dismiss();

    await select("selection-repeat", 6, 12);
    await expectQuote("passed");
    await select("selection-repeat", 19, 25);
    await page.waitForTimeout(240); // The displayed quote is identical; inspect the POST.
    await chip.getByTestId("float-comment").click();
    const repeated = await saveAndSubmit();
    assert.equal(repeated.selector.exact, "passed");
    assert.equal(repeated.selector.start_utf16, 19);
    assert.equal(repeated.selector.end_utf16, 25);
    assert.equal(repeated.block_id, "selection-cases");

    await arm();
    await page.locator("details.cf-tools").evaluate((el) => { el.open = true; });
    await select("selection-repeat", 6, 12);
    const tool = page.getByTestId("tool-add-text");
    await tool.hover();
    await page.mouse.down();
    await page.evaluate(() => {
      getSelection().removeAllRanges();
      document.dispatchEvent(new Event("selectionchange"));
    });
    await page.waitForTimeout(240);
    await page.mouse.up();
    const stashed = await saveAndSubmit();
    assert.equal(stashed.selector.exact, "passed");
    assert.equal(stashed.selector.start_utf16, 6);
    assert.equal(stashed.selector.end_utf16, 12);

    await arm();
    await page.locator("details.cf-tools").evaluate((el) => { el.open = true; });
    await select("selection-repeat", 6, 12);
    await tool.dispatchEvent("pointerdown", { button: 0 });
    await page.evaluate(() => {
      getSelection().removeAllRanges();
      document.dispatchEvent(new Event("selectionchange"));
    });
    await tool.dispatchEvent("pointerdown", { button: 0 });
    await tool.dispatchEvent("click");
    await page.getByText("Select text inside one reviewable block, then add a note.").waitFor();
    assert.equal(await page.getByTestId("composer").count(), 0, "Invalid toolbar capture reused an older stash");
    await select("selection-limit", 0, 17);
    await tool.dispatchEvent("pointerdown", { button: 0 });
    await tool.dispatchEvent("click");
    await page.getByText("Selected text is too long. Select at most 16 characters.").waitFor();
    assert.equal(await page.getByTestId("composer").count(), 0, "Oversized live toolbar fallback opened a composer");
    assert.equal(await chip.count(), 0, "Oversized toolbar fallback pinned a stale selection");
    assert.deepEqual(errors, []);
    await assertNoPolicyViolations(page, "selection lifecycle");
    process.stdout.write("selection lifecycle: invalidation, recovery, limits, occurrence and toolbar anchors passed\n");
  } finally {
    await context.close();
  }
}
