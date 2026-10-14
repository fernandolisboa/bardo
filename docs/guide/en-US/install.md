---
id: install
title: Installing Bardo
group: getting-started
---

# Installing Bardo

Bardo installs like any Windows app, for your Windows user only: it needs no administrator rights, and nothing it installs reaches the other users of the computer.

<a id="download"></a>
## Downloading

Each version of Bardo is a release on [GitHub](https://github.com/fernandolisboa/bardo/releases), with one file to download: `Bardo-Setup-<version>-x64.exe`. Bardo runs on 64-bit Windows 10 and 11.

Next to it, a `.sha256` file holds the installer's SHA-256 checksum. To check the download, run `Get-FileHash Bardo-Setup-<version>-x64.exe` in PowerShell and compare the two.

<a id="smartscreen"></a>
## "Windows protected your PC"

The installer is not signed with a code-signing certificate yet, so the first time you open it, Windows SmartScreen may stop it with **Windows protected your PC**. Select **More info**, check that the file is the one you downloaded, then select **Run anyway**.

<a id="setup"></a>
## Setting it up

The installer speaks English or Portuguese, following Windows. It asks:

- **Where**: by default `%LOCALAPPDATA%\Programs\Bardo`.
- **A desktop shortcut**, if you want one. The Start menu always gets one.
- **Publish even when Bardo is closed?** Asked on the first install only, and unticked unless you tick it. Ticked, Bardo turns its background agent on the next time it opens, as **Publish while Bardo is closed** does. You can change it at any time in [Settings › Publishing](background-agent.md#turn-on).

FFmpeg, which Bardo reads, previews and renders video with, comes inside the installer: there is nothing else to install. Your API keys come next: see [Your API keys](api-keys.md).

<a id="update"></a>
## Updating

Run the installer of the newer version over the one you have. It keeps the folder, your settings and your choice about the background agent, and closes Bardo and its agent first if they are running. Bardo shows its version in [Settings › About](about.md#version).

<a id="uninstall"></a>
## Uninstalling

Uninstall Bardo from Windows Settings › Apps › Installed apps. Close Bardo first; the uninstaller asks if it is still open. It turns the background agent off and removes it from Windows, then removes the program.

Your projects, exports, settings, automatic backups and the keys in Windows Credential Manager stay where they are, so installing Bardo again finds them. See [Where your data and keys live](data-and-keys.md).
