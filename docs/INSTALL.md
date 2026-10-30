---
title: Install and first run
---

# Install and first run

tuldMi runs on **Windows 11 (x64)**. Windows 10 is unverified: it should work, but it hasn't been tested. macOS and Linux are not supported yet.

## 1. Download

Get `tuldMi_<version>_x64-setup.exe` and its `.sha256` file from the
[latest release](https://github.com/spilloid/tuldMi/releases/latest). The installer is about 7 MB:
it does not include a transcription model. You choose and download one during setup.

## 2. Verify it (optional, recommended)

The installer, the app and the uninstaller are Authenticode-signed (`CN=Joseph Spillers`, Azure
Artifact Signing, RFC 3161 timestamped). In PowerShell, from your Downloads folder:

```powershell
Get-AuthenticodeSignature .\tuldMi_*_x64-setup.exe | Format-List Status, SignerCertificate
(Get-FileHash .\tuldMi_*_x64-setup.exe -Algorithm SHA256).Hash
```

`Status` should be `Valid`, and the hash should match the published `.sha256` file.

Because tuldMi is a new publisher, Windows SmartScreen may still show a reputation prompt for the
first downloads. A valid signature from the publisher above is the thing to check.

## 3. Install

Run the installer. It installs for your user account only and does not need administrator
rights.

## 4. Set it up

The first launch walks you through seven short steps:

1. **Welcome**: what tuldMi does.
2. **What's captured**: your microphone is **Me**; whatever your computer plays (for example the
   other side of a call) is **Remote**. Only speech is kept.
3. **Privacy & storage**: where your data lives, and how long audio is kept (don't keep, 7 days,
   30 days or forever; transcripts are kept until you delete them).
4. **Consent**: recording conversations is regulated. You confirm that you are responsible for
   the consent laws and workplace policies that apply to you.
5. **Devices**: pick a microphone and an output device, with a live level test. Nothing is
   stored during the test.
6. **Transcription**: choose a model to download (74 MB to 547 MB; `base.en`, 141 MB, is the
   recommended default). This is the only network request tuldMi makes, and the file is checked
   against its SHA-256 hash. Capture works while the model downloads; speech waits in a queue.
7. **Ready**.

## Everyday use

- tuldMi lives in the **system tray**. The icon shows whether it is recording, paused, or has a
  device problem.
- **Pause** from the tray, the main window, or the global shortcut **Ctrl+Shift+`**. Paused means
  the devices are closed and nothing is captured.
- **Closing the window keeps tuldMi listening.** The first time, it tells you so and offers to
  pause. Use **Quit tuldMi** in the tray menu to stop it completely.
- **Search** with **Ctrl+K** or **/**. Filter by Me or Remote and by date, then jump straight to the
  moment.
- **Export** to Markdown or JSON, or delete one conversation, its audio, or everything, from
  Settings → Privacy & data.

## Removing tuldMi

Uninstall it from Windows Settings → Apps. Your data lives in
`%APPDATA%\com.spillerstech.tuldmi\`. Settings → Privacy & data → *Delete all history* removes
every transcript and recording but keeps your settings and downloaded models; to remove
everything, delete that folder after uninstalling.
