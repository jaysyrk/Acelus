import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..", "..");

const triple = execFileSync("rustc", ["-vV"], { encoding: "utf8" })
  .split("\n")
  .find((line) => line.startsWith("host:"))
  ?.slice("host:".length)
  .trim();

if (!triple) {
  console.error("could not determine the rust host triple");
  process.exit(1);
}

const windows = triple.includes("windows");
const name = windows ? "acelusd.exe" : "acelusd";

execFileSync("cargo", ["build", "--release", "-p", "acelusd"], {
  cwd: root,
  stdio: "inherit",
});

const destination = join(here, "..", "src-tauri", "binaries");
mkdirSync(destination, { recursive: true });

const suffixed = windows ? `acelusd-${triple}.exe` : `acelusd-${triple}`;
copyFileSync(join(root, "target", "release", name), join(destination, suffixed));

console.log(`sidecar ready: binaries/${suffixed}`);
