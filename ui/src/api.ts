export type Json = Record<string, unknown>;

export interface DaemonEvent {
  method: string;
  params: Json;
}

export interface RpcFailure {
  code: number;
  message: string;
}

export class RpcError extends Error {
  readonly code: number;

  constructor(failure: RpcFailure) {
    super(failure.message);
    this.name = "RpcError";
    this.code = failure.code;
  }
}

export const codes = {
  authRequired: -32010,
  authExpired: -32011,
  xboxAccountMissing: -32012,
  xboxChildAccount: -32013,
  xboxRegionUnavailable: -32014,
  xboxAccountBanned: -32015,
  xboxAdultVerificationRequired: -32016,
  entitlementMissing: -32017,
  entitlementSignatureInvalid: -32018,
  azureAppUnapproved: -32019,
  azureAppMissing: -32020,
  credentialStoreUnavailable: -32021,
  notFound: -32001,
  alreadyExists: -32002,
  network: -32003,
  integrityFailed: -32004,
} as const;

type Listener = (event: DaemonEvent) => void;

const listeners = new Set<Listener>();

export function onDaemonEvent(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function emitDaemonEvent(event: DaemonEvent): void {
  for (const listener of listeners) listener(event);
}

function inTauri(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export async function openExternal(url: string): Promise<boolean> {
  if (!inTauri()) {
    window.open(url, "_blank", "noreferrer");
    return true;
  }
  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
    return true;
  } catch {
    return false;
  }
}

export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return fallbackCopy(text);
  }
}

function fallbackCopy(text: string): boolean {
  const holder = document.createElement("textarea");
  holder.value = text;
  holder.setAttribute("readonly", "");
  holder.style.position = "fixed";
  holder.style.opacity = "0";
  document.body.appendChild(holder);
  holder.select();

  let copied = false;
  try {
    copied = document.execCommand("copy");
  } catch {
    copied = false;
  }

  holder.remove();
  return copied;
}

interface Backend {
  connect(): Promise<boolean>;
  rpc(method: string, params: Json): Promise<Json>;
}

let backend: Promise<Backend> | null = null;

async function load(): Promise<Backend> {
  if (inTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    const { listen } = await import("@tauri-apps/api/event");

    await listen<DaemonEvent>("daemon", (event) => emitDaemonEvent(event.payload));

    return {
      connect: () => invoke<boolean>("connect"),
      rpc: (method, params) => invoke<Json>("rpc", { method, params }),
    };
  }

  const { mockBackend } = await import("./mock");
  return mockBackend();
}

function resolveBackend(): Promise<Backend> {
  if (!backend) backend = load();
  return backend;
}

export async function connect(): Promise<boolean> {
  try {
    return await (await resolveBackend()).connect();
  } catch {
    return false;
  }
}

export async function rpc<T = Json>(method: string, params: Json = {}): Promise<T> {
  try {
    return (await (await resolveBackend()).rpc(method, params)) as T;
  } catch (raw) {
    throw toRpcError(raw);
  }
}

function toRpcError(raw: unknown): RpcError {
  if (raw instanceof RpcError) return raw;
  if (raw && typeof raw === "object" && "message" in raw) {
    const failure = raw as Partial<RpcFailure>;
    return new RpcError({
      code: typeof failure.code === "number" ? failure.code : 0,
      message: String(failure.message ?? "the request failed"),
    });
  }
  return new RpcError({ code: 0, message: String(raw) });
}

export function explain(error: unknown): { title: string; detail: string; link?: string } {
  const failure = toRpcError(error);
  switch (failure.code) {
    case codes.credentialStoreUnavailable:
      return {
        title: "Signed in, but the account could not be saved",
        detail:
          "Acelus keeps your sign in in the system keyring, and it could not be written. On Linux that usually means no keyring service is running or it is locked. The sign in itself worked.",
      };
    case codes.azureAppMissing:
      return {
        title: "This copy of Acelus cannot sign in yet",
        detail:
          "Signing in to Minecraft needs a Microsoft application that Mojang has approved, and this build was not given one. Whoever built or shared this copy has to set that up; there is nothing to fix on your side.",
      };
    case codes.azureAppUnapproved:
      return {
        title: "Mojang has not approved this application yet",
        detail:
          "Sign in works up to the last step. Minecraft's API refuses unapproved applications, so approval has to land before an account can be added.",
        link: "https://aka.ms/mce-reviewappid",
      };
    case codes.xboxAccountMissing:
      return {
        title: "This Microsoft account has no Xbox profile",
        detail: "Sign in once at xbox.com to create one, then try again.",
        link: "https://www.xbox.com",
      };
    case codes.xboxChildAccount:
      return {
        title: "This is a child account",
        detail: "It has to be added to a Microsoft family group before it can sign in.",
      };
    case codes.xboxRegionUnavailable:
      return {
        title: "Xbox Live is not available in this region",
        detail: "The account's country or region cannot use Xbox Live, which Minecraft sign in requires.",
      };
    case codes.xboxAccountBanned:
      return { title: "This Xbox account is banned", detail: "Xbox Live refused the sign in." };
    case codes.xboxAdultVerificationRequired:
      return {
        title: "This account needs adult verification",
        detail: "Complete verification on the Xbox website, then try again.",
      };
    case codes.entitlementMissing:
      return {
        title: "This account does not own Minecraft: Java Edition",
        detail: "Acelus will not launch without a verified entitlement.",
      };
    case codes.entitlementSignatureInvalid:
      return {
        title: "The ownership proof could not be verified",
        detail:
          "The entitlement signature did not match Mojang's key. The response was altered in transit, or Mojang rotated the key.",
      };
    case codes.network:
      return { title: "Could not reach the network", detail: failure.message };
    case codes.integrityFailed:
      return {
        title: "A file did not match its published digest",
        detail: failure.message,
      };
    default:
      return { title: "Something went wrong", detail: failure.message };
  }
}
