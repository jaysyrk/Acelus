import { connect } from "./api";
import "./styles.css";
import { h, icons, svg } from "./dom";
import { renderAccounts } from "./views/accounts";
import { renderConsole, watchLogs } from "./views/console";
import { renderInstances, watchInstalls } from "./views/instances";
import { applyTheme, currentTheme, nextTheme, themeLabel, type Theme } from "./theme";

type Route = "instances" | "accounts" | "console";

const routes: Array<{ id: Route; label: string; icon: string }> = [
  { id: "instances", label: "Instances", icon: icons.boxes },
  { id: "accounts", label: "Accounts", icon: icons.user },
  { id: "console", label: "Console", icon: icons.terminal },
];

let current: Route = routeFromHash();
let live = false;

function routeFromHash(): Route {
  const name = location.hash.replace(/^#\/?/, "");
  return routes.some((route) => route.id === name) ? (name as Route) : "instances";
}

const main = h("main", {});
const navButtons = new Map<Route, HTMLButtonElement>();
const statusDot = h("span", { class: "dot" });
const statusText = h("span", {}, "connecting");

let theme: Theme = currentTheme();
const themeButton = h(
  "button",
  {
    class: "nav-item",
    title: "Switch between system, dark and light",
    onclick: () => {
      theme = nextTheme(theme);
      applyTheme(theme);
      themeLabelNode.textContent = themeLabel(theme);
    },
  },
  svg(icons.contrast, 17),
) as HTMLButtonElement;
const themeLabelNode = h("span", {}, themeLabel(theme));
themeButton.appendChild(themeLabelNode);

function mark(): void {
  for (const [id, button] of navButtons) {
    if (id === current) button.setAttribute("aria-current", "page");
    else button.removeAttribute("aria-current");
  }
}

function draw(): void {
  mark();
  if (current === "instances") void renderInstances(main, go("console"));
  else if (current === "accounts") void renderAccounts(main);
  else void renderConsole(main);
}

function go(route: Route): () => void {
  return () => {
    if (current === route) return;
    current = route;
    if (routeFromHash() !== route) location.hash = `#/${route}`;
    draw();
  };
}

function redrawIfViewing(route: Route): () => void {
  return () => {
    if (current === route) draw();
  };
}

function brandMark(): SVGSVGElement {
  const element = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  element.setAttribute("viewBox", "0 0 24 24");
  element.setAttribute("width", "26");
  element.setAttribute("height", "26");

  const shades = ["#2a7a60", "#38b184", "#50e7ac"];
  shades.forEach((fill, index) => {
    const shape = document.createElementNS("http://www.w3.org/2000/svg", "path");
    const drop = (shades.length - 1 - index) * 3.1;
    shape.setAttribute(
      "d",
      `M12 ${3.4 + drop} L20.4 ${18.6 + drop} H16.6 L12 ${9.6 + drop} L7.4 ${18.6 + drop} H3.6 Z`,
    );
    shape.setAttribute("fill", fill);
    element.appendChild(shape);
  });

  return element;
}

function sidebar(): HTMLElement {
  const nav = h(
    "aside",
    { class: "sidebar" },
    h("div", { class: "brand" }, brandMark(), h("span", { class: "brand-name" }, "Acelus")),
  );

  for (const route of routes) {
    const button = h(
      "button",
      { class: "nav-item", onclick: go(route.id) },
      svg(route.icon, 17),
      route.label,
    ) as HTMLButtonElement;
    navButtons.set(route.id, button);
    nav.appendChild(button);
  }

  nav.appendChild(h("div", { class: "nav-spacer" }));
  nav.appendChild(themeButton);
  nav.appendChild(h("div", { class: "daemon-state" }, statusDot, statusText));
  return nav;
}

function setStatus(connected: boolean): void {
  live = connected;
  statusDot.className = `dot ${connected ? "live" : "down"}`;
  statusText.textContent = connected ? "acelusd running" : "acelusd unavailable";
}

async function start(): Promise<void> {
  applyTheme(theme);

  const app = document.getElementById("app");
  if (!app) return;

  app.appendChild(sidebar());
  app.appendChild(main);

  window.addEventListener("hashchange", () => {
    const route = routeFromHash();
    if (route !== current) {
      current = route;
      draw();
    }
  });

  watchInstalls(redrawIfViewing("instances"));
  watchLogs(redrawIfViewing("console"));

  setStatus(await connect());
  draw();

  if (!live) {
    window.setInterval(async () => {
      if (!live) {
        setStatus(await connect());
        if (live) draw();
      }
    }, 3000);
  }
}

void start();
