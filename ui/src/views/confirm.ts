import { h } from "../dom";

export function confirmAction(options: {
  title: string;
  detail: string;
  confirmLabel: string;
  destructive?: boolean;
  onConfirm: () => void | Promise<void>;
}): void {
  const scrim = h("div", { class: "scrim" });
  const close = () => scrim.remove();

  const confirm = h(
    "button",
    {
      class: options.destructive ? "btn danger" : "btn primary",
      onclick: async (event: Event) => {
        (event.currentTarget as HTMLButtonElement).disabled = true;
        await options.onConfirm();
        close();
      },
    },
    options.confirmLabel,
  );

  scrim.appendChild(
    h(
      "div",
      { class: "dialog", style: "width:min(430px,100%)" },
      h("div", { class: "dialog-head" }, h("h2", {}, options.title)),
      h(
        "div",
        { class: "dialog-body" },
        h("p", { style: "margin:0;color:var(--muted);font-size:13.5px" }, options.detail),
      ),
      h("div", { class: "dialog-foot" }, h("button", { class: "btn", onclick: close }, "Cancel"), confirm),
    ),
  );

  scrim.addEventListener("click", (event) => {
    if (event.target === scrim) close();
  });
  document.addEventListener("keydown", function escape(event) {
    if (event.key === "Escape") {
      close();
      document.removeEventListener("keydown", escape);
    }
  });

  document.body.appendChild(scrim);
  (confirm as HTMLButtonElement).focus();
}
