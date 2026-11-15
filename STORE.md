# Publishing Askeri Ruler to the Microsoft Store

## 1. One-time setup in Partner Center

1. Sign in at <https://partner.microsoft.com/dashboard> with a Microsoft account and register as a
   **Windows & Xbox (Apps and games)** developer — individual or company. Check the current
   registration terms and fee when you sign up.
2. **Apps and games → New product → MSIX or PWA app**, and **reserve the product name**
   (the name shown in the Store, e.g. "Askeri Ruler"). It must be unique in the Store.
3. Open the new product → **Product management → Product identity**. Copy these three values into
   [`packaging/package.json`](packaging/package.json):

   | Partner Center field            | package.json key       |
   |---------------------------------|------------------------|
   | Package/Identity/Name           | `IdentityName`         |
   | Package/Identity/Publisher      | `Publisher`            |
   | Package/Properties/PublisherDisplayName | `PublisherDisplayName` |

   Also set `DisplayName` to the exact reserved product name.

## 2. Build the package

```powershell
powershell -ExecutionPolicy Bypass -File packaging\build-msix.ps1
```

Output: `target\msix\AskeriRuller_<version>_x64.msix`. You do **not** need to sign it — the Store
signs packages itself.

For every update, bump `version` in `Cargo.toml` (e.g. `0.1.0` → `0.2.0`); the package version
becomes `0.2.0.0`. The Store rejects a version that is not higher than the previous submission.

Optional local check with the Windows App Certification Kit (run as administrator):

```powershell
& "C:\Program Files (x86)\Windows Kits\10\App Certification Kit\appcert.exe" test `
    -appxpackagepath target\msix\AskeriRuller_0.1.0.0_x64.msix -reportoutputpath target\msix\wack.xml
```

## 3. Create a submission

| Section | What to enter |
|---|---|
| **Pricing and availability** | Markets, price (free or paid), visibility. |
| **Properties** | Category: *Productivity* or *Utilities & tools* (see the note on medical use below). Privacy policy URL: host [`PRIVACY.md`](PRIVACY.md) somewhere public (e.g. GitHub Pages, your website) and paste the link. |
| **Age ratings** | Fill in the questionnaire — no user interaction, no data sharing, no purchases → typically rated 3+/Everyone. |
| **Packages** | Upload the `.msix` from step 2. Device family: Windows 10/11 Desktop. |
| **Store listings** (English) | Description, short description, at least one **screenshot** (PNG, ≥ 1366×768) showing the line over a dental image, and keywords. The 300×300 logo is at `target\msix\listing\logo-300x300.png`. |
| **Submission options → Notes for certification** | See text below. |

### Restricted capability justification (`runFullTrust`)

> Askeri Ruler is a classic Win32 desktop application packaged with MSIX. It needs runFullTrust to
> draw a transparent always-on-top overlay across all monitors, register a global hotkey, and read
> mouse movement while the user positions and rotates the guide line.

### Notes for certification

> The app runs in the notification area (tray icon). Press Ctrl+Shift+X to show a red guide line that
> follows the mouse. Hold the right mouse button and move the mouse up/down to rotate it; release to
> lock it in place (the line then lets clicks through to other apps). Press Esc to remove all lines.
> Right-click the tray icon for Settings and Exit. While lines are visible, the app uses a low-level
> keyboard hook solely to detect the Esc key; no input is stored or transmitted, and the app makes no
> network connections.

## 4. Note on dental / medical use

The Store treats software that claims to diagnose or treat medical conditions with extra scrutiny,
and some markets regulate it as a medical device. Describe Askeri Ruler as a **visual guide /
on-screen ruler** (e.g. "align and compare features on any image, including dental X-rays") rather
than as a diagnostic or measurement-certified medical tool, unless you intend to go through the
relevant regulatory process.

## 5. After certification

Certification usually takes a few hours to a few days. Once published, the Store installs and
updates the app; "Start with Windows" uses the package's startup task, which users can also toggle
in **Settings → Apps → Startup**.
