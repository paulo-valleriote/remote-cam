// Chromium capture harness entry point, 64-bit consumer (T035).
//
// **NON-PRODUCTION** — F0 risk-reduction spike.
//
// Starts the local harness page and opens it in the default browser, which on a
// 64-bit Windows installation is the 64-bit build. The page does the measuring;
// this entry point only decides *which* browser opens it and when to stop:
//
//   node apps/browser/capture.mjs --backend mf_virtual_camera \
//     --application-id chrome --duration-s 1800 --output-dir <dir> \
//     --product-name "Google Chrome" --version 142.0.7444.60
//
// Exit codes: 0 when a capture (or a recorded failure) exists, 1 when nothing
// was produced, so `run_matrix.py` never treats silence as evidence.

import { spawn } from "node:child_process";
import { writeFile } from "node:fs/promises";
import { join } from "node:path";

import { parseArgs, requireIdentity, startServer } from "./server.mjs";

async function openInDefaultBrowser(url) {
  if (process.platform === "win32") {
    spawn("cmd", ["/c", "start", "", url], { detached: true, stdio: "ignore" }).unref();
  } else if (process.platform === "darwin") {
    spawn("open", [url], { detached: true, stdio: "ignore" }).unref();
  } else {
    spawn("xdg-open", [url], { detached: true, stdio: "ignore" }).unref();
  }
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  requireIdentity(options);
  const harness = await startServer(options);
  const url = `${harness.url}?duration=${options.durationS}&application=${options.applicationId}&backend=${options.backend}`;

  console.log(`open: ${url}`);
  console.log("pick RemoteCam Virtual Camera (Spike) in the permission prompt, then press Start");
  if (options.open) await openInDefaultBrowser(url);

  const outcome = await harness.done;
  harness.server.close();

  if (outcome.outcome === "uploaded") {
    console.log(`capture written to ${outcome.directory} (${outcome.observations} frame observations)`);
    return 0;
  }
  if (outcome.outcome === "failed") {
    // A camera that cannot be opened is a matrix failure with its own outcome,
    // not a harness error: record it and let the matrix judge it.
    console.log(`camera not visible: ${outcome.failure.reason}`);
    return 0;
  }
  await writeFile(
    join(harness.directory, "harness_error.json"),
    `${JSON.stringify({ application_id: options.applicationId, outcome: outcome.outcome }, null, 2)}\n`,
  );
  console.error(`no capture was produced (${outcome.outcome})`);
  return 1;
}

main().then((code) => process.exit(code));
