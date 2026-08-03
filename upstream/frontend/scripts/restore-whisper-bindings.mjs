#!/usr/bin/env node
// Ensures the vendored whisper-rs-sys bindings.rs is in place.
//
// Why this script exists
// ----------------------
// whisper-rs-sys generates `bindings.rs` via bindgen at build time, which
// requires LLVM and a working C compiler. The project commits a copy of
// the generated file alongside the crate's other generated artifacts in
// the cargo registry cache. Build is forced to use that file via
// WHISPER_DONT_GENERATE_BINDINGS=1.
//
// The catch: the bindings.rs lives in the cargo registry cache
// (%CARGO_HOME%/registry/src/...), which is treated as a transient cache.
// Anything that invalidates that cache (cargo clean, a wipe of cargo home,
// switching toolchain, etc.) forces a re-download of whisper-rs-sys, and
// the hand-curated bindings.rs is gone — the build then fails until
// bindgen can re-generate it.
//
// This script copies the project's committed copy of bindings.rs back into
// the cargo registry cache so the build keeps working.
//
// Usage:
//   node scripts/restore-whisper-bindings.mjs
//
// Environment:
//   CARGO_HOME   (optional) override cargo home location
//   PROJECT_ROOT (optional) override the project root to locate the
//                         vendored bindings file

import { promises as fs } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const cargoHome = process.env.CARGO_HOME || join(homedir(), ".cargo");
const scriptDir = dirname(fileURLToPath(import.meta.url));
// Walk up from the script location until we find frontend/src-tauri/vendor
async function findProjectRoot() {
    if (process.env.PROJECT_ROOT) return process.env.PROJECT_ROOT;
    let dir = scriptDir;
    for (let i = 0; i < 5; i++) {
        const candidate = join(
            dir,
            "frontend",
            "src-tauri",
            "vendor",
            "whisper-rs-sys",
            "src",
            "bindings.rs"
        );
        if (await exists(candidate)) return dir;
        const parent = dirname(dir);
        if (parent === dir) break;
        dir = parent;
    }
    return process.cwd();
}

const projectRoot = await findProjectRoot();
const vendoredSrc = join(
    projectRoot,
    "frontend",
    "src-tauri",
    "vendor",
    "whisper-rs-sys",
    "src",
    "bindings.rs"
);
const targetRel = join(
    "registry",
    "src",
    "index.crates.io-1949cf8c6b5b557f",
    "whisper-rs-sys-0.11.1",
    "src",
    "bindings.rs"
);
const target = join(cargoHome, targetRel);

async function exists(p) {
    try {
        await fs.access(p);
        return true;
    } catch {
        return false;
    }
}

async function main() {
    if (!(await exists(vendoredSrc))) {
        console.error(`Missing vendored bindings at ${vendoredSrc}`);
        console.error(
            "The file is expected to be committed to the repo. Re-add it before running this script."
        );
        process.exit(1);
    }

    if (await exists(target)) {
        const [a, b] = await Promise.all([fs.readFile(vendoredSrc), fs.readFile(target)]);
        if (a.equals(b)) {
            console.log("Bindings already in place; no action needed.");
            return;
        }
        console.log("Bindings differ from vendored copy; restoring...");
    } else {
        console.log("Bindings missing in cargo registry cache; restoring...");
    }

    await fs.mkdir(join(target, ".."), { recursive: true });
    await fs.copyFile(vendoredSrc, target);
    console.log(`Restored ${target}`);
    console.log(
        "Now run the build with WHISPER_DONT_GENERATE_BINDINGS=1 to use the vendored bindings."
    );
}

main().catch((err) => {
    console.error(err);
    process.exit(1);
});
