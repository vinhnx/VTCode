import { constants } from "node:fs";
import { open, realpath } from "node:fs/promises";
import { extname, isAbsolute, join, relative } from "node:path";
import { createInterface } from "node:readline";
import { setTimeout } from "node:timers/promises";
import { chromium } from "playwright";

const origin = "http://vtcode-preview.invalid";
const mount = "/build";
const viewport = { width: 1280, height: 800 };
const maxRequestBytes = 1024 * 1024;
const maxAssetBytes = 32 * 1024 * 1024;
const assetTypes = new Map([
  [".html", "text/html"], [".css", "text/css"], [".js", "text/javascript"],
  [".mjs", "text/javascript"], [".json", "application/json"], [".wasm", "application/wasm"],
  [".svg", "image/svg+xml"], [".png", "image/png"], [".jpeg", "image/jpeg"],
  [".jpg", "image/jpeg"], [".webp", "image/webp"], [".avif", "image/avif"],
  [".gif", "image/gif"], [".ico", "image/x-icon"], [".woff", "font/woff"],
  [".woff2", "font/woff2"], [".txt", "text/plain"], [".mp4", "video/mp4"],
]);

function validPoint(point) {
  if (!Number.isInteger(point?.x) || !Number.isInteger(point?.y)
      || point.x < 0 || point.y < 0 || point.x >= viewport.width || point.y >= viewport.height) {
    throw new Error("Invalid coordinate");
  }
  return point;
}

function keyName(key) {
  const aliases = new Map([
    ["ctrl", "Control"], ["control", "Control"], ["alt", "Alt"],
    ["shift", "Shift"], ["super", "Meta"], ["meta", "Meta"],
    ["return", "Enter"], ["esc", "Escape"], ["space", " "],
    ["up", "ArrowUp"], ["down", "ArrowDown"], ["left", "ArrowLeft"], ["right", "ArrowRight"],
    ["page_up", "PageUp"], ["page_down", "PageDown"],
  ]);
  return aliases.get(key.toLowerCase()) ?? key;
}

function deliver(page) {
  let cursor = { x: 0, y: 0 };
  let leftHeld = false;

  async function move(point) {
    const next = validPoint(point);
    await page.mouse.move(next.x, next.y);
    cursor = next;
  }

  async function withKeys(keys, action) {
    const held = [];
    try {
      for (const key of keys) {
        const name = keyName(key);
        await page.keyboard.down(name);
        held.push(name);
      }
      return await action();
    } finally {
      for (const name of held.reverse()) {
        await page.keyboard.up(name);
      }
    }
  }

  async function image(clip) {
    const data = await page.screenshot({ type: "png", clip, timeout: 5000 });
    return { kind: "screenshot", data: data.toString("base64") };
  }

  return async action => {
    if (page.isClosed() || !page.url().startsWith(`${origin}/`)) {
      throw new Error("Target page is unavailable");
    }
    const modifiers = action.modifiers ?? [];
    switch (action.kind) {
      case "screenshot":
        return image();
      case "zoom": {
        const topLeft = validPoint(action.top_left);
        const bottomRight = action.bottom_right;
        if (!Number.isInteger(bottomRight?.x) || !Number.isInteger(bottomRight?.y)
            || bottomRight.x <= topLeft.x || bottomRight.y <= topLeft.y
            || bottomRight.x > viewport.width || bottomRight.y > viewport.height) {
          throw new Error("Invalid screenshot region");
        }
        return image({ ...topLeft, width: bottomRight.x - topLeft.x, height: bottomRight.y - topLeft.y });
      }
      case "cursor_position":
        return { kind: "cursor_position", ...cursor };
      case "move":
        await withKeys(modifiers, () => move(action.at));
        break;
      case "click":
        if (leftHeld || !["left", "middle", "right"].includes(action.button)) {
          throw new Error("Unsupported mouse button or state");
        }
        await withKeys(modifiers, async () => {
          if (action.at) await move(action.at);
          await page.mouse.click(cursor.x, cursor.y, { button: action.button, clickCount: action.count });
        });
        break;
      case "left_mouse_down":
        if (leftHeld) throw new Error("Mouse button already held");
        await page.mouse.down();
        leftHeld = true;
        break;
      case "left_mouse_up":
        if (!leftHeld) throw new Error("Mouse button already released");
        await page.mouse.up();
        leftHeld = false;
        break;
      case "drag":
        if (leftHeld) throw new Error("Mouse button already held");
        for (const point of action.path) validPoint(point);
        await withKeys(modifiers, async () => {
          const [start, ...rest] = action.path;
          await move(start);
          await page.mouse.down();
          try {
            for (const point of rest) await move(point);
          } finally {
            await page.mouse.up();
          }
        });
        break;
      case "scroll_pixels":
      case "scroll_ticks": {
        const ticks = action.amount * 100;
        const dx = action.kind === "scroll_pixels" ? action.x : (
          action.direction === "left" ? -ticks : action.direction === "right" ? ticks : 0
        );
        const dy = action.kind === "scroll_pixels" ? action.y : (
          action.direction === "up" ? -ticks : action.direction === "down" ? ticks : 0
        );
        await withKeys(modifiers, async () => {
          if (action.at) await move(action.at);
          await page.mouse.wheel(dx, dy);
        });
        break;
      }
      case "key": {
        const chord = action.keys.map(keyName).join("+");
        for (let repeat = 0; repeat < action.repeat; repeat++) await page.keyboard.press(chord);
        break;
      }
      case "hold_key":
        await withKeys(action.keys, () => setTimeout(action.milliseconds));
        break;
      case "type":
        await page.keyboard.insertText(action.text);
        break;
      case "wait":
        await setTimeout(action.milliseconds);
        break;
      default:
        throw new Error("Unsupported computer action");
    }
    return { kind: "done" };
  };
}

async function serveAsset(route) {
  const url = new URL(route.request().url());
  if (url.origin !== origin || !["GET", "HEAD"].includes(route.request().method())) {
    await route.abort();
    return;
  }
  let body;
  let file;
  try {
    const pathname = decodeURIComponent(url.pathname);
    const asset = pathname.endsWith("/") ? `${pathname}index.html` : pathname;
    const handle = await open(join(mount, asset), constants.O_RDONLY | constants.O_NONBLOCK);
    try {
      file = await realpath(`/proc/self/fd/${handle.fd}`);
      const within = relative(mount, file);
      if (!within || within === ".." || within.startsWith("../") || isAbsolute(within)) {
        throw new Error("Asset is outside the build");
      }
      const metadata = await handle.stat();
      if (!metadata.isFile() || metadata.size > maxAssetBytes) throw new Error("Invalid asset");
      body = Buffer.alloc(metadata.size);
      let offset = 0;
      while (offset < body.length) {
        const { bytesRead } = await handle.read(body, offset, body.length - offset, offset);
        if (bytesRead === 0) throw new Error("Asset changed during read");
        offset += bytesRead;
      }
    } finally {
      await handle.close();
    }
  } catch {
    await route.fulfill({ status: 404, body: "Not found" });
    return;
  }
  await route.fulfill({
    body,
    contentType: assetTypes.get(extname(file).toLowerCase()) ?? "application/octet-stream",
  });
}

async function run() {
  const browser = await chromium.launch({ args: ["--disable-dev-shm-usage"], handleSIGINT: false, handleSIGTERM: false });
  try {
    const context = await browser.newContext({ viewport, serviceWorkers: "block", acceptDownloads: false });
    await context.route("**/*", serveAsset);
    const page = await context.newPage();
    context.on("page", candidate => {
      if (candidate !== page) void candidate.close().catch(() => {});
    });
    const loaded = await page.goto(`${origin}/`, { waitUntil: "domcontentloaded", timeout: 5000 });
    if (!loaded?.ok()) throw new Error("Build entry point is unavailable");

    const execute = deliver(page);
    process.stdout.write('{"kind":"ready"}\n');
    for await (const line of createInterface({ input: process.stdin, crlfDelay: Infinity })) {
      try {
        if (Buffer.byteLength(line) > maxRequestBytes) throw new Error("Action is too large");
        const response = await execute(JSON.parse(line));
        process.stdout.write(`${JSON.stringify(response)}\n`);
      } catch {
        process.stdout.write('{"kind":"failed"}\n');
        return;
      }
    }
  } finally {
    await browser.close();
  }
}

run().catch(() => {
  process.stdout.write('{"kind":"failed"}\n');
  process.exitCode = 1;
});
