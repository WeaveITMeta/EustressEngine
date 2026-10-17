// /player/host.js: loads the browser Player for a /play/<id> page.
//
// Contract (reviewed with Multiplayer):
//   start({ container, simId?, apiBase, join? }) -> Promise<controller>
//   controller.enterFullscreen(), controller.stop(), controller.state
//
// `join` is the eustress-player://join/... link the Play page minted for this
// click, with its `ticket` and `api` already appended, exactly as the desktop
// Player receives it. It goes straight to the wasm entry (crates/player-web
// `start_player`), which runs the same split_link_extras, JoinLink parsing
// and check_api the desktop does, and requires the host's pin. This file
// never keeps it, and never puts it in an event, status text, URL, history,
// storage or the console. A published world played alone (`simId` without
// `join`) is not in this build yet, and reads as unavailable.
//
// A page runs one session: start() runs once, and joining again reloads the
// page.
//
// Status goes to the container as a "eustress-player-status" event whose
// detail is { state, message }. Every message is fixed text chosen here; the
// wasm module reports a state name only, so nothing it says can carry link
// text.
//
// NOT RUN against a host yet: the builds come from crates/player-web.

// The two builds sit beside this file (/player/webgpu/ and /player/webgl2/ on
// the site), so the bundle can move to another host, such as R2 when a
// build passes the 25 MiB Pages cap, with no change here.
const BUILDS = { webgpu: new URL("webgpu/", import.meta.url), webgl2: new URL("webgl2/", import.meta.url) };
// The Player's own assets, shared by both builds: its default source and the
// bundled library (`bundled://`) a Space can name.
const ASSETS = new URL("assets/", import.meta.url);
const GLUE = "eustress_player.js"; // wasm-bindgen glue module
const ENTRY = "start_player";
const SIM_ID = /^[0-9a-f-]{8,64}$/; // the same rule the commerce contract uses
const API_ORIGINS = new Set(["https://api.eustress.dev", "https://api-staging.eustress.dev"]);

const MESSAGES = {
  loading: "Loading the browser Player.",
  unavailable: "The browser Player is not on this site yet. Use the desktop Player.",
  // A browser without WebGPU, on a site with only the WebGPU build.
  "unsupported-browser": "This browser cannot run the browser Player yet. Use a browser with WebGPU, or the desktop Player.",
  starting: "Starting.",
  joining: "Joining the session.",
  "in-world": "Playing.",
  stopped: "Stopped.",
  "load-failed": "The browser Player could not load.",
  "join-failed": "Could not join the session.",
  disconnected: "The connection to the host was lost.",
  // A host's refusal. The wire carries the host's own text, which any host can
  // set, so the entry maps it to one of these (in Rust, beside the desktop
  // Player's notice) and this page shows only fixed text.
  // client join_notice.rs: the host's "This ticket was already used." plus GALLERY_REJOIN.
  "refused:ticket-used": "This ticket was already used. Join again from the gallery.",
  "refused:not-available": "This session is not available right now.",
  // The Worker's ADULT_CONTENT_OFF (infrastructure/cloudflare/api/src/adult_content.mjs), which the host passes on.
  "refused:adult-content-off": "This listing is adult content. Turn on Adult content in your Profile to see it.",
  // The Worker's AGE_21_REQUIRED (adult_content.mjs), which the host passes on.
  "refused:age-21": "This listing is for adults 21 and over. It opens once your verified age is 21.",
  "refused:protocol": "This host runs a different version.",
  // The host's "This session is full (N of M players)." (eustress-networking session.rs), without the count,
  // plus the desktop's hint (client join_notice.rs).
  "refused:full": "This session is full. Try again when someone leaves.",
  refused: "The host did not let you in.",
};
// The states the wasm module may report through onState. Any other refusal
// shows the general "refused" text; any other string is ignored.
const ENTRY_STATES = new Set([
  "starting",
  "joining",
  "in-world",
  "stopped",
  "join-failed",
  "disconnected",
  ...Object.keys(MESSAGES).filter((s) => s.startsWith("refused")),
]);

// The state to show for one the module reports, or null to ignore it. A
// failure after the world opened is a lost connection, never a failed join.
function accepted(state, current) {
  if (typeof state !== "string") return null;
  if (!ENTRY_STATES.has(state)) return state.startsWith("refused:") ? "refused" : null;
  if (state === "join-failed" && (current === "in-world" || current === "disconnected")) return "disconnected";
  return state;
}

export function chooseBuild(nav = globalThis.navigator) {
  return nav && nav.gpu ? "webgpu" : "webgl2";
}

function report(container, controller, state) {
  if (controller) controller.state = state;
  container.dispatchEvent(new CustomEvent("eustress-player-status", { detail: { state, message: MESSAGES[state] } }));
}

function inert(state) {
  return { state, enterFullscreen: async () => {}, stop: () => {} };
}

// Whether the build at `base` is deployed. Probed first, so a missing bundle
// is a clear status and not a module error.
async function deployed(base) {
  try {
    return (await fetch(new URL(GLUE, base), { method: "HEAD", cache: "no-cache" })).ok;
  } catch {
    return false;
  }
}

let started = false;

export async function start({ container, simId, apiBase, join } = {}) {
  if (started) throw new TypeError("start() runs once per page; reload the page to join again");
  if (!(container instanceof HTMLElement)) throw new TypeError("start() needs a container element");
  if (simId !== undefined && (typeof simId !== "string" || !SIM_ID.test(simId))) {
    throw new TypeError("simId must be a Gallery id");
  }
  if (join !== undefined && (typeof join !== "string" || !join.startsWith("eustress-player://join/"))) {
    throw new TypeError("join must be an eustress-player join link"); // fixed text: never echo the link
  }
  if (simId === undefined && join === undefined) throw new TypeError("start() needs a simId or a join link");
  if (!API_ORIGINS.has(new URL(apiBase).origin)) throw new TypeError("apiBase must be one of the Eustress API origins");
  started = true;

  const build = chooseBuild();
  const base = BUILDS[build];
  report(container, null, "loading");

  if (!(await deployed(base))) {
    const state = build === "webgl2" && (await deployed(BUILDS.webgpu)) ? "unsupported-browser" : "unavailable";
    report(container, null, state);
    return inert(state);
  }

  let mod;
  try {
    mod = await import(new URL(GLUE, base).href);
    if (typeof mod.default === "function") await mod.default(); // wasm-bindgen init
  } catch {
    report(container, null, "load-failed");
    return inert("load-failed");
  }
  // A published world alone (simId, no join) is not in this build yet.
  if (typeof mod[ENTRY] !== "function" || join === undefined) {
    report(container, null, "unavailable");
    return inert("unavailable");
  }

  const controller = {
    state: "starting",
    async enterFullscreen() {
      if (container.requestFullscreen) await container.requestFullscreen();
    },
    stop() {},
  };
  const onState = (state) => {
    const shown = accepted(state, controller.state);
    if (shown) report(container, controller, shown);
  };
  report(container, controller, "starting");

  let handle;
  try {
    handle = await mod[ENTRY]({ canvasParent: container, simId: simId ?? null, apiBase, assetBase: ASSETS.href, join, onState });
  } catch {
    report(container, controller, "join-failed");
    return inert("join-failed");
  }

  let stopped = false;
  controller.stop = () => {
    if (stopped) return;
    stopped = true;
    removeEventListener("pagehide", controller.stop);
    try {
      if (handle && typeof handle.stop === "function") handle.stop();
    } finally {
      report(container, controller, "stopped");
    }
  };
  // A page that closes or navigates away gives the host a clean close, not a
  // timeout.
  addEventListener("pagehide", controller.stop);
  return controller;
}
