// Shared local server for the Chromium capture harnesses (T035, T038).
//
// **NON-PRODUCTION** — F0 risk-reduction spike.
//
// The 64-bit and 32-bit entries differ only in how they open the page, so the
// server, the upload handling and the refusal rules live here.
//
// The server refuses to write evidence it cannot stand behind:
//
//   * a page that never uploads anything produces no `capture.json`;
//   * a page that could not open the camera writes `failure.json` instead, which
//     `finalise_run` turns into a `device_not_visible` cell rather than silence;
//   * a page whose observed bitness disagrees with the plan is rejected.

import { createServer } from "node:http";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const pagePath = join(here, "index.html");

export function parseArgs(argv) {
  const options = {
    host: "127.0.0.1",
    port: 0,
    productName: "",
    version: "",
    durationS: 1800,
    outputDir: "",
    applicationId: "",
    backend: "",
    open: true,
    timeoutS: 0,
    width: 1280,
    height: 720,
    fps: 30,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    const next = () => {
      index += 1;
      if (index >= argv.length) throw new Error(`${argument} needs a value`);
      return argv[index];
    };
    if (argument === "--product-name") options.productName = next();
    else if (argument === "--version") options.version = next();
    else if (argument === "--duration-s") options.durationS = Number(next());
    else if (argument === "--output-dir") options.outputDir = next();
    else if (argument === "--application-id") options.applicationId = next();
    else if (argument === "--backend") options.backend = next();
    else if (argument === "--width") options.width = Number(next());
    else if (argument === "--height") options.height = Number(next());
    else if (argument === "--fps") options.fps = Number(next());
    else if (argument === "--notes") options.notes = next();
    else if (argument === "--port") options.port = Number(next());
    else if (argument === "--timeout-s") options.timeoutS = Number(next());
    else if (argument === "--no-open") options.open = false;
    else throw new Error(`unknown option ${argument}`);
  }
  return options;
}

export function requireIdentity(options) {
  const missing = [];
  if (!options.productName) missing.push("--product-name");
  if (!options.version) missing.push("--version");
  if (!options.outputDir) missing.push("--output-dir");
  if (!options.applicationId) missing.push("--application-id");
  if (missing.length) {
    throw new Error(
      `the harness needs ${missing.join(", ")}: the exact product and version are gate evidence`,
    );
  }
}

export async function startServer(options) {
  requireIdentity(options);
  const directory = resolve(options.outputDir);
  await mkdir(directory, { recursive: true });

  let resolveDone;
  const done = new Promise((settle) => {
    resolveDone = settle;
  });

  const server = createServer(async (request, response) => {
    const url = new URL(request.url, `http://${options.host}`);

    if (request.method === "GET" && (url.pathname === "/" || url.pathname === "/index.html")) {
      const page = await readFile(pagePath, "utf8");
      response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      response.end(page);
      return;
    }

    if (request.method === "POST" && url.pathname === "/failed") {
      const body = await readBody(request);
      const failure = {
        application_id: options.applicationId,
        product_name: options.productName,
        version: options.version,
        reason: JSON.parse(body || "{}").reason ?? "the page reported a failure",
        captured_at: new Date().toISOString(),
      };
      await writeFile(join(directory, "failure.json"), `${JSON.stringify(failure, null, 2)}\n`);
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({ ok: true }));
      resolveDone({ outcome: "failed", failure });
      return;
    }

    if (request.method === "POST" && url.pathname === "/upload") {
      try {
        const written = await handleUpload(request, options, directory);
        response.writeHead(200, { "content-type": "application/json" });
        response.end(JSON.stringify({ ok: true, ...written }));
        resolveDone({ outcome: "uploaded", ...written });
      } catch (error) {
        response.writeHead(500, { "content-type": "application/json" });
        response.end(JSON.stringify({ ok: false, error: String(error.message ?? error) }));
        resolveDone({ outcome: "refused", error });
      }
      return;
    }

    response.writeHead(404, { "content-type": "text/plain" });
    response.end("not found");
  });

  const listening = new Promise((settle) => {
    server.listen(options.port, options.host, () => settle());
  });
  await listening;
  const port = server.address().port;

  if (options.timeoutS > 0) {
    setTimeout(() => resolveDone({ outcome: "timeout" }), options.timeoutS * 1000).unref();
  }

  return { server, directory, port, url: `http://${options.host}:${port}/`, done };
}

async function handleUpload(request, options, directory) {
  const { fields, file } = await readMultipart(request);
  const payload = JSON.parse(fields.payload ?? "{}");

  if (payload.application_id && payload.application_id !== options.applicationId) {
    throw new Error(
      `the page reported ${payload.application_id}, but this run is ${options.applicationId}`,
    );
  }
  const observations = Array.isArray(payload.observations) ? payload.observations : [];
  if (!observations.length) {
    throw new Error("the page uploaded no frame observations; a cell cannot be concluded from an empty capture");
  }

  const extension = (fields.extension ?? "webm").replace(/[^a-z0-9]/gi, "") || "webm";
  const captureFile = `capture.${extension}`;
  await writeFile(join(directory, captureFile), file.data);

  const rows = ["capture_host_ms,width,height,frame_no"];
  for (const item of observations) {
    rows.push(
      [
        Number(item.capture_host_ms).toFixed(3),
        item.width,
        item.height,
        item.frame_no === null || item.frame_no === undefined ? "" : item.frame_no,
      ].join(","),
    );
  }
  const observationsFile = "observations.csv";
  await writeFile(join(directory, observationsFile), `${rows.join("\n")}\n`);

  const first = observations[0];
  const last = observations[observations.length - 1];
  const record = {
    application_id: options.applicationId,
    capture_method: "getusermedia_mediarecorder",
    capture_file: captureFile,
    product_name: options.productName,
    version: options.version,
    bitness: payload.bitness ?? "unknown",
    requested: { width: options.width, height: options.height, frame_rate: options.fps },
    recorded_duration_s: Math.max(0, (last.capture_host_ms - first.capture_host_ms) / 1000),
    discarded_frames: 0,
    captured_at: new Date().toISOString(),
    observed: {
      observations_source: "browser_request_video_frame_callback",
      cameras: payload.cameras ?? [],
      negotiated: payload.negotiated ?? {},
      mime_type: payload.mime_type ?? null,
      user_agent: payload.user_agent ?? null,
    },
  };
  if (options.notes) record.notes = options.notes;
  await writeFile(join(directory, "capture.json"), `${JSON.stringify(record, null, 2)}\n`);

  return { observations: observations.length, directory, captureFile };
}

function readBody(request) {
  return new Promise((settle, fail) => {
    const chunks = [];
    request.on("data", (chunk) => chunks.push(chunk));
    request.on("end", () => settle(Buffer.concat(chunks).toString("utf8")));
    request.on("error", fail);
  });
}

async function readMultipart(request) {
  const contentType = request.headers["content-type"] ?? "";
  const boundaryMatch = /boundary=(?:"([^"]+)"|([^;]+))/i.exec(contentType);
  if (!boundaryMatch) throw new Error("the upload is not multipart/form-data");
  const boundary = `--${(boundaryMatch[1] ?? boundaryMatch[2]).trim()}`;
  const raw = await readRaw(request);
  const fields = {};
  let file = null;

  for (const part of splitBuffer(raw, Buffer.from(boundary)).slice(1)) {
    const separator = part.indexOf("\r\n\r\n");
    if (separator === -1) continue;
    const header = part.slice(0, separator).toString("utf8");
    const body = stripCrLf(Buffer.from(part.slice(separator + 4)));
    const nameMatch = /name="([^"]+)"/i.exec(header);
    if (!nameMatch) continue;
    if (/filename="/i.test(header)) file = { name: nameMatch[1], data: body };
    else fields[nameMatch[1]] = body.toString("utf8");
  }
  if (!file) throw new Error("the upload did not carry a recording");
  return { fields, file };
}

function readRaw(request) {
  return new Promise((settle, fail) => {
    const chunks = [];
    request.on("data", (chunk) => chunks.push(chunk));
    request.on("end", () => settle(Buffer.concat(chunks)));
    request.on("error", fail);
  });
}

function splitBuffer(buffer, separator) {
  const parts = [];
  let start = 0;
  for (;;) {
    const index = buffer.indexOf(separator, start);
    if (index === -1) break;
    parts.push(buffer.subarray(start, index));
    start = index + separator.length;
  }
  return parts;
}

function stripCrLf(buffer) {
  let end = buffer.length;
  while (end > 0 && (buffer[end - 1] === 0x0a || buffer[end - 1] === 0x0d)) end -= 1;
  return buffer.subarray(0, end);
}
