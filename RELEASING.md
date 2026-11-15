# Releasing

Askeri Ruler ships two ways:

| Channel | What | How |
|---|---|---|
| **GitHub Releases** | `AskeriRuler-Setup-<version>.exe` installer, portable `.zip`, `SHA256SUMS.txt` | Automatic: push a version tag (below) |
| **Microsoft Store** | `.msix` | Manual upload to Partner Center — see [STORE.md](STORE.md) |

## Publish a GitHub release

1. Bump `version` in `Cargo.toml` (e.g. `0.1.0` → `0.2.0`) and commit.
   Run `cargo build` once so `Cargo.lock` picks up the new version, and commit that too.
2. Create a tag **`v` + the same version**, e.g. `v0.2.0`, on that commit and push it:
   - **GitHub Desktop:** *History* tab → right-click the commit → *Create Tag…* → `v0.2.0` →
     *Push origin* (tags are pushed with it).
   - **Command line:** `git tag v0.2.0` then `git push origin v0.2.0`.
3. The [Release workflow](.github/workflows/release.yml) builds on GitHub's Windows runner,
   runs the tests, and publishes a release with the installer, portable zip and checksums
   (release notes are generated from the commits). Watch it under the repo's *Actions* tab.
   The `.msix` for the Store is attached to the workflow run as an artifact.

If the tag does not match `Cargo.toml`, the workflow fails before building — delete the tag,
fix the version and tag again.

## Build locally

```powershell
winget install JRSoftware.InnoSetup                                   # once
powershell -ExecutionPolicy Bypass -File installer\build-installer.ps1 # -> dist\
```

## The installer

- Per-user install (no administrator prompt) to `%LOCALAPPDATA%\Programs\Askeri Ruler`,
  Start menu shortcut, entry in *Settings → Apps*.
- Optional "Start when I sign in" task (same Run-key entry the app's own setting uses, with
  `--autostart`, so a second copy started at login exits silently).
- Upgrading or uninstalling closes a running copy cleanly first. Uninstall removes the
  autostart entry; settings in `%APPDATA%\AskeriRuller` are kept.

## Code signing

Unsigned downloads work, but Windows SmartScreen shows "Windows protected your PC" and some
antivirus tools are more suspicious of unsigned apps (this one uses a keyboard hook for Esc).
Signing fixes the publisher name and lets reputation build up over time.

Both build scripts accept a sign command containing `{file}`:

```powershell
$env:SIGN_COMMAND = 'signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 <your cert options> "{file}"'
powershell -ExecutionPolicy Bypass -File installer\build-installer.ps1
```

The app exe is signed before it goes into the installer, then the installer itself is signed.
Always keep the `/tr` timestamp so signatures stay valid after the certificate expires.

Certificate options (private keys must live in hardware or a cloud HSM since 2023):

- **Azure Trusted Signing** (Microsoft's cloud signing, low monthly cost) — eligibility depends
  on country and whether you sign as an individual or organization; check current availability.
  Has an official GitHub Action and a `signtool` plugin.
- **OV code-signing certificate with cloud signing** (e.g. SSL.com eSigner, Certum, Sectigo) —
  available to individuals in most countries; use the provider's CLI or signtool integration.

To sign in GitHub Actions, add a step before "Build installer" in
`.github/workflows/release.yml` that sets up your provider's tool (credentials from repository
secrets) and exports `SIGN_COMMAND` via `$env:GITHUB_ENV`.
