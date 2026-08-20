type Child = Node | string | null | undefined | false;

type Attrs = Record<string, string | number | boolean | null | undefined | EventListener>;

export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Attrs = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);

  for (const [key, value] of Object.entries(attrs)) {
    if (value === null || value === undefined || value === false) continue;
    if (key.startsWith("on") && typeof value === "function") {
      element.addEventListener(key.slice(2).toLowerCase(), value as EventListener);
    } else if (key === "class") {
      element.className = String(value);
    } else {
      element.setAttribute(key, String(value));
    }
  }

  append(element, children);
  return element;
}

export function append(parent: Node, children: Child[]): void {
  for (const child of children) {
    if (child === null || child === undefined || child === false) continue;
    parent.appendChild(typeof child === "string" ? document.createTextNode(child) : child);
  }
}

export function svg(path: string, size = 16): SVGSVGElement {
  const element = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  element.setAttribute("viewBox", "0 0 24 24");
  element.setAttribute("width", String(size));
  element.setAttribute("height", String(size));
  element.setAttribute("fill", "none");
  element.setAttribute("stroke", "currentColor");
  element.setAttribute("stroke-width", "1.9");
  element.setAttribute("stroke-linecap", "round");
  element.setAttribute("stroke-linejoin", "round");

  const shape = document.createElementNS("http://www.w3.org/2000/svg", "path");
  shape.setAttribute("d", path);
  element.appendChild(shape);
  return element;
}

export const icons = {
  boxes: "M3 8l9-5 9 5v8l-9 5-9-5V8zM3 8l9 5 9-5M12 13v8",
  user: "M4 20v-1a5 5 0 0 1 5-5h6a5 5 0 0 1 5 5v1M12 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8z",
  terminal: "M5 6l5 5-5 5M12.5 17H19",
  play: "M7 4.5v15l12-7.5-12-7.5z",
  stop: "M6 6h12v12H6z",
  plus: "M12 5v14M5 12h14",
  download: "M12 4v11m0 0l4-4m-4 4l-4-4M5 19h14",
  trash: "M4 7h16M9 7V5h6v2M6 7l1 13h10l1-13",
  refresh: "M3 12a9 9 0 0 1 15-6.7L21 8M21 4v4h-4M21 12a9 9 0 0 1-15 6.7L3 16M3 20v-4h4",
  external: "M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5",
  copy: "M9 9h10v10a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1V9zM5 15V5a1 1 0 0 1 1-1h9",
  contrast: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 3v18",
};

export function clear(node: Node): void {
  while (node.firstChild) node.removeChild(node.firstChild);
}

export function bytes(value: number): string {
  if (value < 1024) return `${value} B`;
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let size = value / 1024;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size < 10 ? size.toFixed(1) : Math.round(size)} ${units[unit]}`;
}

export function ago(seconds: number | null): string {
  if (!seconds) return "never played";
  const delta = Math.max(0, Date.now() / 1000 - seconds);
  const steps: Array<[number, string]> = [
    [60, "second"],
    [60, "minute"],
    [24, "hour"],
    [7, "day"],
    [4.34, "week"],
    [12, "month"],
  ];

  let value = delta;
  let label = "second";
  for (const [size, name] of steps) {
    if (value < size) {
      label = name;
      break;
    }
    value /= size;
    label = name;
  }

  const rounded = Math.max(1, Math.floor(value));
  return `${rounded} ${label}${rounded === 1 ? "" : "s"} ago`;
}
