import { render } from "preact";
import { Chrome } from "./chrome";
import { CHROME_ROOT_ID, DOCUMENT_ROOT_ID, readChromeConfig } from "./contracts";
import { enhanceDocument } from "./enhance";
import "./styles.css";
import "./figure.css";

void import("./fonts").then(({ installFonts }) => installFonts());

const chromeRoot = document.getElementById(CHROME_ROOT_ID);
const documentRoot = document.getElementById(DOCUMENT_ROOT_ID);

if (!(chromeRoot instanceof HTMLElement) || !(documentRoot instanceof HTMLElement)) {
  throw new Error("cf-present document and chrome roots are required");
}

const config = readChromeConfig(chromeRoot);
enhanceDocument(documentRoot);
render(<Chrome config={config} documentRoot={documentRoot} />, chromeRoot);
