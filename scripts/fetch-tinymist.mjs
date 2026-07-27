/**
 * Fetch the tinymist binary for the current platform and place it where
 * Tauri's `externalBin` expects it: `src-tauri/binaries/tinymist-<target-triple>`
 * (no extension on macOS/Linux; `.exe` appended on Windows is handled by Tauri).
 *
 * Run automatically via `beforeBuildCommand`, or manually with `node scripts/fetch-tinymist.mjs`.
 * Re-runs are no-ops when the binary already exists.
 *
 * @see https://v2.tauri.app/develop/sidecar/
 */
import {
  existsSync,
  mkdirSync,
  renameSync,
  rmSync,
  statSync,
} from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'
import { tmpdir } from 'node:os'

const __dirname = dirname(fileURLToPath(import.meta.url))
const ROOT = join(__dirname, '..')
const BIN_DIR = join(ROOT, 'src-tauri', 'binaries')
const VERSION = 'v0.15.2'

// Map process.platform/process.arch to a Rust target triple + the GitHub
// release asset suffix. tinymist publishes these combinations.
const TARGETS = {
  darwin: {
    arm64: { triple: 'aarch64-apple-darwin', asset: 'aarch64-apple-darwin' },
    x64: { triple: 'x86_64-apple-darwin', asset: 'x86_64-apple-darwin' },
  },
  win32: {
    arm64: { triple: 'aarch64-pc-windows-msvc', asset: 'aarch64-pc-windows-msvc' },
    x64: { triple: 'x86_64-pc-windows-msvc', asset: 'x86_64-pc-windows-msvc' },
  },
  linux: {
    arm64: { triple: 'aarch64-unknown-linux-gnu', asset: 'aarch64-unknown-linux-gnu' },
    x64: { triple: 'x86_64-unknown-linux-gnu', asset: 'x86_64-unknown-linux-gnu' },
  },
}

const plat = TARGETS[process.platform]?.[process.arch]
if (!plat) {
  console.error(`fetch-tinymist: unsupported platform ${process.platform}/${process.arch}`)
  process.exit(0) // non-fatal: skip on unsupported hosts (e.g. CI cross-builds)
}

// On Windows, Tauri expects `<name>-<triple>.exe`.
const ext = process.platform === 'win32' ? '.exe' : ''
const dest = join(BIN_DIR, `tinymist-${plat.triple}${ext}`)

if (existsSync(dest)) {
  // Skip if already present (avoid re-downloading on every build).
  console.log(`fetch-tinymist: ${plat.triple} already present, skipping.`)
  process.exit(0)
}

mkdirSync(BIN_DIR, { recursive: true })

const assetName = `tinymist-${plat.asset}.tar.gz`
const url = `https://github.com/Myriad-Dreamin/tinymist/releases/download/${VERSION}/${assetName}`
const tmpTar = join(tmpdir(), assetName)

console.log(`fetch-tinymist: downloading ${VERSION} for ${plat.triple}`)
console.log(`  ${url}`)

// curl is universally available on dev machines (macOS/Linux/Windows+git-bash).
const curl = spawnSync('curl', ['-fL', '--retry', '3', '-o', tmpTar, url], { stdio: 'inherit' })
if (curl.status !== 0) {
  console.error(`fetch-tinymist: download failed (curl exit ${String(curl.status)})`)
  process.exit(1)
}

// Extract. The tarball contains `tinymist-<asset>/tinymist`.
const extractDir = join(tmpdir(), `tinymist-extract-${String(process.pid)}`)
mkdirSync(extractDir, { recursive: true })
const tar = spawnSync('tar', ['-xzf', tmpTar, '-C', extractDir], { stdio: 'inherit' })
if (tar.status !== 0) {
  console.error(`fetch-tinymist: extract failed (tar exit ${String(tar.status)})`)
  process.exit(1)
}

// Locate the binary inside the extracted dir.
const extractedBin = join(extractDir, `tinymist-${plat.asset}`, `tinymist${ext}`)
if (!existsSync(extractedBin)) {
  console.error(`fetch-tinymist: extracted binary not found at ${extractedBin}`)
  process.exit(1)
}

renameSync(extractedBin, dest)
if (process.platform !== 'win32') {
  spawnSync('chmod', ['+x', dest])
}

// Cleanup.
rmSync(extractDir, { recursive: true, force: true })
rmSync(tmpTar, { force: true })

const sizeMB = Math.round(statSync(dest).size / 1024 / 1024)
console.log(`fetch-tinymist: placed ${dest} (${String(sizeMB)} MB)`)
