/** Include offscreen sandboxed frames in the compositor's capture viewport. */
export async function prepareExportCapture(page) {
  await page.waitForSelector("html[data-cf-ready='true']");
  const viewport = page.viewportSize();
  const height = await page.evaluate(() => document.documentElement.scrollHeight);
  await page.setViewportSize({ width: viewport.width, height: Math.max(viewport.height, height) });
  await page.evaluate(async () => {
    await document.fonts.ready;
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  });
}
