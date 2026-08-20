import { explain, onDaemonEvent, rpc } from "../api";
import { clear, h, icons, svg } from "../dom";

interface Account {
  uuid: string;
  name: string;
  skinUrl?: string | null;
  entitlementVerified: boolean;
  expired: boolean;
}

export async function renderAccounts(toolbar: HTMLElement, root: HTMLElement): Promise<void> {
  const reload = () => void renderAccounts(toolbar, root);
  clear(toolbar);
  clear(root);

  toolbar.appendChild(h("h1", {}, "Accounts"));
  toolbar.appendChild(h("span", { class: "spacer" }));
  toolbar.appendChild(
    h("button", { class: "btn accent", onclick: () => beginLogin(reload) }, svg(icons.plus, 13), "Sign in"),
  );

  let accounts: Account[] = [];
  let active: string | null = null;
  try {
    const reply = await rpc<{ accounts: Account[]; active: string | null }>("account.list");
    accounts = reply.accounts ?? [];
    active = reply.active ?? null;
  } catch (error) {
    root.appendChild(failure(error));
    return;
  }

  if (accounts.length === 0) {
    root.appendChild(
      h(
        "div",
        { class: "blank" },
        h("strong", {}, "No accounts"),
        "Launching needs an account that owns the game, proven by signature.",
      ),
    );
    return;
  }

  const list = h("div", {});
  for (const account of accounts) {
    list.appendChild(
      h(
        "div",
        { class: "account" },
        h("img", {
          class: "avatar",
          alt: "",
          src: account.skinUrl ?? `https://crafatar.com/avatars/${account.uuid}?size=76&overlay`,
        }),
        h(
          "div",
          { style: "flex:1;min-width:0" },
          h("div", { style: "font-weight:550" }, account.name),
          h("div", { class: "data dim", style: "font-size:11px" }, account.uuid),
        ),
        account.entitlementVerified
          ? h("span", { class: "pill on" }, "owns the game")
          : h("span", { class: "pill" }, "unverified"),
        account.expired ? h("span", { class: "pill" }, "expired") : null,
        account.uuid === active
          ? h("span", { class: "pill on" }, "active")
          : h(
              "button",
              {
                class: "btn quiet",
                onclick: async () => {
                  await rpc("account.select", { uuid: account.uuid });
                  reload();
                },
              },
              "Use",
            ),
        h(
          "button",
          {
            class: "btn quiet bad",
            title: "Sign out and erase stored credentials",
            onclick: async () => {
              await rpc("account.remove", { uuid: account.uuid });
              reload();
            },
          },
          svg(icons.trash, 13),
        ),
      ),
    );
  }
  root.appendChild(list);
}

function beginLogin(reload: () => void): void {
  const scrim = h("div", { class: "scrim" });
  const body = h("div", { class: "sheet-body" });

  const dialog = h(
    "div",
    { class: "sheet", style: "width:min(440px,100%)" },
    h("div", { class: "sheet-head" }, "Sign in"),
    body,
    h(
      "div",
      { class: "sheet-foot" },
      h("button", { class: "btn", onclick: () => close() }, "Close"),
    ),
  );

  let stop: (() => void) | null = null;
  const close = () => {
    stop?.();
    scrim.remove();
    reload();
  };

  body.appendChild(h("div", { class: "row" }, h("span", { class: "spin" }), "Asking Microsoft for a code"));

  scrim.appendChild(dialog);
  scrim.addEventListener("click", (event) => {
    if (event.target === scrim) close();
  });
  document.body.appendChild(scrim);

  void (async () => {
    let begun: { userCode: string; verificationUri: string };
    try {
      begun = await rpc<{ userCode: string; verificationUri: string }>("account.beginLogin");
    } catch (error) {
      clear(body);
      body.appendChild(failure(error));
      return;
    }

    clear(body);
    body.appendChild(
      h("p", { style: "margin:0;color:var(--muted)" }, "Open this page and enter the code:"),
    );
    body.appendChild(h("div", { class: "code" }, begun.userCode));
    body.appendChild(
      h(
        "div",
        { class: "row", style: "justify-content:center" },
        h(
          "a",
          { class: "btn", href: begun.verificationUri, target: "_blank", rel: "noreferrer" },
          svg(icons.external, 13),
          begun.verificationUri.replace(/^https?:\/\//, ""),
        ),
        h(
          "button",
          {
            class: "btn quiet",
            onclick: () => void navigator.clipboard?.writeText(begun.userCode),
          },
          svg(icons.copy, 13),
          "Copy code",
        ),
      ),
    );
    body.appendChild(
      h("div", { class: "row", style: "justify-content:center;color:var(--faint);font-size:12.5px" },
        h("span", { class: "spin" }), "Waiting for the sign in to finish"),
    );

    stop = onDaemonEvent((event) => {
      if (event.method !== "account.loginComplete") return;
      clear(body);
      const error = event.params["error"];
      if (error) {
        body.appendChild(failure(error));
        return;
      }
      const account = event.params["account"] as { name?: string } | null;
      body.appendChild(
        h(
          "div",
          { class: "notice" },
          h("strong", {}, `Signed in as ${account?.name ?? "your account"}`),
          "Acelus stored the refresh token in your system keyring.",
        ),
      );
    });
  })();
}

function failure(error: unknown): HTMLElement {
  const detail = explain(error);
  return h(
    "div",
    { class: "note bad" },
    h("strong", {}, detail.title),
    detail.detail,
    detail.link
      ? h(
          "div",
          { style: "margin-top:8px" },
          h("a", { class: "btn", href: detail.link, target: "_blank", rel: "noreferrer" },
            svg(icons.external, 13), "Open the approval form"),
        )
      : null,
  );
}
