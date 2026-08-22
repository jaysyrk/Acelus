import { connect, rpc } from "./api";
import "./styles.css";
import { h, icons, svg } from "./dom";
import { applyTheme, currentTheme, nextTheme, themeLabel, type Theme } from "./theme";
import { renderAccounts } from "./views/accounts";
import { renderConsole, stopWatching } from "./views/console";
import { renderInstances, watchInstalls, whenAccountsNeeded } from "./views/instances";

type Route = "instances" | "accounts" | "console";

const order: Route[] = ["instances", "accounts", "console"];
const labels: Record<Route, string> = {
  instances: "Instances",
  accounts: "Accounts",
  console: "Console",
};
const glyphs: Record<Route, string> = {
  instances: icons.boxes,
  accounts: icons.user,
  console: icons.terminal,
};

let current: Route = routeFromHash();
let live = false;
let theme: Theme = currentTheme();

const toolbar = h("div", { class: "toolbar" });
const body = h("div", { class: "scroll" });
const navButtons = new Map<Route, HTMLButtonElement>();
const counts = new Map<Route, HTMLElement>();
const stateDot = h("span", { class: "dot" });
const stateText = h("span", {}, "connecting");

function routeFromHash(): Route {
  const name = location.hash.replace(/^#\/?/, "");
  return order.includes(name as Route) ? (name as Route) : "instances";
}

function draw(): void {
  for (const [id, button] of navButtons) {
    if (id === current) button.setAttribute("aria-current", "page");
    else button.removeAttribute("aria-current");
  }

  if (current !== "console") stopWatching();

  if (current === "instances") void renderInstances(toolbar, body, go("console"));
  else if (current === "accounts") void renderAccounts(toolbar, body);
  else void renderConsole(toolbar, body);

  void refreshCounts();
}

function go(route: Route): () => void {
  return () => {
    if (current === route) return;
    current = route;
    if (routeFromHash() !== route) location.hash = `#/${route}`;
    draw();
  };
}

async function refreshCounts(): Promise<void> {
  try {
    const instances = await rpc<{ instances: unknown[] }>("instance.list");
    setCount("instances", instances.instances?.length ?? 0);
  } catch {
    setCount("instances", null);
  }
  try {
    const accounts = await rpc<{ accounts: unknown[] }>("account.list");
    setCount("accounts", accounts.accounts?.length ?? 0);
  } catch {
    setCount("accounts", null);
  }
  try {
    const sessions = await rpc<{ sessions: unknown[] }>("launch.status");
    setCount("console", sessions.sessions?.length ?? 0);
  } catch {
    setCount("console", null);
  }
}

function setCount(route: Route, value: number | null): void {
  const node = counts.get(route);
  if (node) node.textContent = value === null || value === 0 ? "" : String(value);
}

function mark(): SVGSVGElement {
  const element = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  element.setAttribute("viewBox", "0 0 24 24");

  const shades = ["var(--faint)", "var(--muted)", "var(--accent)"];
  shades.forEach((fill, index) => {
    const shape = document.createElementNS("http://www.w3.org/2000/svg", "path");
    const drop = (shades.length - 1 - index) * 3.2;
    shape.setAttribute(
      "d",
      `M12 ${3.2 + drop} L20.6 ${18.6 + drop} H16.8 L12 ${9.4 + drop} L7.2 ${18.6 + drop} H3.4 Z`,
    );
    shape.setAttribute("fill", fill);
    element.appendChild(shape);
  });

  return element;
}

function rail(): HTMLElement {
  const nav = h("aside", { class: "rail" }, h("div", { class: "wordmark" }, mark(), "Acelus"));

  for (const route of order) {
    const count = h("span", { class: "count" });
    counts.set(route, count);
    const button = h(
      "button",
      { class: "nav-item", onclick: go(route) },
      svg(glyphs[route], 14),
      labels[route],
      count,
    ) as HTMLButtonElement;
    navButtons.set(route, button);
    nav.appendChild(button);
  }

  nav.appendChild(h("div", { class: "rail-spacer" }));

  const themeText = h("span", {}, themeLabel(theme));
  nav.appendChild(
    h(
      "button",
      {
        class: "nav-item",
        onclick: () => {
          theme = nextTheme(theme);
          applyTheme(theme);
          themeText.textContent = themeLabel(theme);
        },
      },
      svg(icons.contrast, 14),
      themeText,
    ),
  );

  nav.appendChild(h("div", { class: "rail-foot" }, stateDot, stateText));
  return nav;
}

function setState(connected: boolean): void {
  live = connected;
  stateDot.className = `dot ${connected ? "live" : "down"}`;
  stateText.textContent = connected ? "acelusd" : "no daemon";
}

async function start(): Promise<void> {
  applyTheme(theme);

  const app = document.getElementById("app");
  if (!app) return;

  app.appendChild(rail());
  app.appendChild(h("main", {}, toolbar, body));

  window.addEventListener("hashchange", () => {
    const route = routeFromHash();
    if (route !== current) {
      current = route;
      draw();
    }
  });

  watchInstalls();
  whenAccountsNeeded(go("accounts"));

  setState(await connect());
  draw();

  window.setInterval(async () => {
    if (!live) {
      setState(await connect());
      if (live) draw();
    }
  }, 3000);
}

void start();
