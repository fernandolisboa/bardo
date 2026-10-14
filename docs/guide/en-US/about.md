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

The version of Bardo on this computer. Say it when you report a problem.

<a id="updates"></a>
## Updates

Bardo looks for a newer release on [GitHub](https://github.com/fernandolisboa/bardo/releases) when it opens and once a day while it stays open. When one is out, a bar over the screens says so: **See what's new** opens this section, and **Later** hides the bar until Bardo opens again or a newer release comes out. Nothing is downloaded or installed until you choose to.

- **Check for updates automatically** turns the checks at start and every day off or on. **Check now** checks at once, even when it is off. The line under it says when Bardo last checked and what it found.
- A newer release shows its version, when it was published, what's new in it and a link to its **Release page**.
- **Download** fetches its installer as a job, with its progress here and in [Jobs](jobs.md). The file is checked against the SHA-256 checksum its release publishes; a file that does not match is deleted and never runs. A download cut short leaves nothing behind.
- **Install…** opens a review. **Install and restart** checks the file once more, starts the installer and closes Bardo; the installer updates Bardo without its wizard, keeping your data, settings and the background agent choice, and opens Bardo again when it is done.

Installing waits while jobs run in this window (wait for them, or cancel them in Jobs), during a [restore](backup.md#restore), and while another Bardo window is open. If **Publish while Bardo is closed** is on, the [background agent](background-agent.md) closes for the install too and starts again within 15 minutes.

Checking asks GitHub's public API without any account: no key, no sign-in, and nothing about you or your projects is sent. A copy from the Microsoft Store never checks, since the Store updates it. A copy built from source says when a release is out and links to it: update the source to get it. You can always [update by hand](install.md#update).

<a id="privacy"></a>
## Privacy and YouTube

Bardo uses YouTube API Services, so it asks you to accept its [privacy policy](privacy.md) before any YouTube feature works: saving the YouTube Data API key or YouTube app credentials, connecting a channel, niche research, syncing YouTube posts and uploading. This section shows whether you accepted it, and when.

- **Read the privacy policy** opens it in the Guide; **Privacy policy online** opens the same page on the guide's website, the address to give Google's forms.
- **Accept the privacy policy** records that you accepted this version. When the policy changes, YouTube features wait again until you accept the new one, and a bar over the screens says so.
- The links under it lead to the [YouTube Terms of Service](https://www.youtube.com/t/terms), which you agree to by using Bardo's YouTube features, the [Google Privacy Policy](https://www.google.com/policies/privacy) and your Google account's [access page](https://security.google.com/settings/security/permissions), where you can remove Bardo's access.
- **Delete YouTube data…** asks first, then deletes at once the niche research, theme rankings and post numbers Bardo read from YouTube ([Deleting your YouTube data](privacy.md#delete)). Nothing on YouTube changes.

Bardo also deletes what YouTube's rules no longer allow it to keep, by itself, once a day ([Where it is kept, and for how long](privacy.md#kept)).

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
