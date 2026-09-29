// Readiness only: never launch a browser or install dependencies here.
import { access } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
const candidates = process.platform === 'darwin'
  ? ['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', '/Applications/Chromium.app/Contents/MacOS/Chromium']
  : process.platform === 'win32'
    ? [`${process.env.PROGRAMFILES}/Google/Chrome/Application/chrome.exe`, `${process.env['PROGRAMFILES(X86)']}/Microsoft/Edge/Application/msedge.exe`]
    : ['google-chrome', 'chromium', 'chromium-browser', 'microsoft-edge'];
let found = false;
for (const candidate of candidates) {
  try {
    if (candidate.includes('/')) await access(candidate);
    else execFileSync(candidate, ['--version'], { stdio: 'ignore' });
    process.stdout.write(`installed browser: ${candidate}\n`);
    found = true; break;
  } catch { /* Try the next supported installed browser. */ }
}
if (!found) throw new Error('No supported browser is installed in this sandbox');
