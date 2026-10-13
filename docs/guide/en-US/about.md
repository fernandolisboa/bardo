---
id: about
title: About Bardo
group: reference
place: settings/about
tour: about
---

# About Bardo

[Settings › About](bardo:go/settings/about) says which Bardo you have and what it is built with, and gathers what is needed to report a problem. [Show me the tab](bardo:tour/about).

<a id="version"></a>
## Version

The version of Bardo on this computer. Say it when you report a problem, and compare it with the latest release to know whether to [update](install.md#update).

<a id="ffmpeg"></a>
## FFmpeg

Bardo reads, previews and renders video with [FFmpeg](https://ffmpeg.org), which the installer puts in the `ffmpeg` folder next to Bardo. FFmpeg is free software under the GNU Lesser General Public License version 3 (LGPL v3): Bardo runs it as separate programs and ships it unmodified.

- **FFmpeg source code** downloads the source this build of FFmpeg was made from.
- **License (LGPL v3)** opens the license that comes with it.

<a id="notices"></a>
## Third-party notices

Bardo is built with open-source libraries. **Open the notices** opens one page, installed with Bardo, listing each library and its license. A copy of Bardo built from its source code has no such page.

<a id="diagnostics"></a>
## Diagnostics

When something goes wrong, **Export diagnostics** saves one `.zip` file, wherever you choose, to attach to an issue on [GitHub](https://github.com/fernandolisboa/bardo/issues). It holds:

- Bardo's logs and the background agent's, each with its previous copy;
- `system.txt`: the versions of Bardo, FFmpeg and Windows, the graphics adapter, the layout, theme and language, and the database version;
- `jobs.txt`: how many jobs are in each state, and the last failures with their reasons;
- `accounts.txt`: whether each account is connected or needs to reconnect.

Every key, secret and token Bardo knows is replaced by `[redacted]`, account names and network ids by `[account]`, and your Windows user folder by `%USERPROFILE%`. Nothing is sent anywhere: the file stays where you saved it until you attach it, and you can open it to see what it holds first. It works with Bardo open and the [background agent](background-agent.md) running.

**Open logs folder** opens `%LOCALAPPDATA%\Bardo\logs`, where the logs are ([Logs](data-and-keys.md#logs)).
