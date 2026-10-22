// Chromium capture harness entry point, 32-bit consumer (T038).
//
// **NON-PRODUCTION** — F0 risk-reduction spike.
//
// Identical measurement to `apps/browser/capture.mjs`, with one deliberate
// difference: it never opens the page itself. Auto-opening would launch the
// *default* browser, which on a 64-bit machine is the 64-bit build, and the
// cell would silently test the wrong bitness. The operator opens the printed
// URL in a 32-bit Chromium build.
//
// The page reports the bitness it observes, and the plan's bitness is checked in
// `finalise_run`; a 32-bit consumer that cannot see the device is a matrix
// failure, not a limitation (Q-8).
//
//   node apps/browser32/capture.mjs --backend mf_virtual_camera \
//     --application-id chrome32 --duration-s 1800 --output-dir <dir> \
//     --product-name "Google Chrome (32-bit)" --version 142.0.7444.60

import { writeFile } from "node:fs/promises";
import { join } from "node:path";

import { parseArgs, requireIdentity, startServer } from "../browser/server.mjs";

async function main() {
  const options = parseArgs(process.argv.slice(2));
  requireIdentity(options);
  const harness = await startServer(options);

  const url = `${harness.url}?duration=${options.durationS}&application=${options.applicationId}&backend=${options.backend}`;
  console.log("32-bit consumer run — the page is NOT opened automatically.");
  console.log(`  open this URL in the 32-bit Chromium build: ${url}`);
  console.log("  verify with chrome://version that the build is 32-bit (x86) before starting");
  console.log("  then pick RemoteCam Virtual Camera (Spike) and press Start");

  const outcome = await harness.done;
  harness.server.close();

  if (outcome.outcome === "uploaded") {
    console.log(`capture written to ${outcome.directory} (${outcome.observations} frame observations)`);
    return 0;
  }
  if (outcome.outcome === "failed") {
    console.log(`camera not visible to the 32-bit consumer: ${outcome.failure.reason}`);
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
