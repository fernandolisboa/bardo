---
id: background-agent
title: Publishing while Bardo is closed
group: publishing
place: settings/publishing
tour: background-agent
---

# Publishing while Bardo is closed

Instagram takes no publish time from apps, so Bardo posts a scheduled Reel itself at its time ([Scheduling](uploading.md#schedule)). On its own, that needs Bardo open at that time. [Settings › Publishing](bardo:go/settings/publishing) can add a lightweight **background agent** that sends those posts while Bardo is closed. It is off until you turn it on. [Show me the tab](bardo:tour/background-agent).

<a id="what"></a>
## What the agent sends

The agent sends what Bardo posts at a due time: Instagram Reels. YouTube publishes scheduled videos by itself, even with your computer off, and TikTok drafts are posted from the TikTok app, so the agent has nothing to do for them.

It uses the same data, accounts and keys as Bardo: nothing is copied, and your keys and tokens stay in Windows Credential Manager. Its upload shows in [Jobs](jobs.md) and on the project's Publish stage like any other.

<a id="turn-on"></a>
## Turning it on and off

**Publish while Bardo is closed** asks Windows to start the agent each time you sign in, as you, without administrator rights, and starts it right away. Turning it off stops the agent and removes it from Windows. If Windows refuses either change, Bardo says so: an agent that could not be set up stays off, and one Windows did not let Bardo remove stops by itself and is removed the next time Bardo opens.

<a id="status"></a>
## Is it running?

Below the switch, Bardo says whether the agent is off, running, or on but not running right now (for example, it stopped after an error). Windows starts it again the next time you sign in, and checks on it every 15 minutes; **Start now** starts it at once.

<a id="limits"></a>
## What it needs

- The computer must be on, and you signed in to Windows. A locked screen is fine; the agent does not wake a sleeping computer, and it does not run while you are signed out.
- While Bardo is open, Bardo sends the posts itself and the agent waits. Each post goes out once, whichever of the two sends it.
- A post whose time passes while neither runs is missed, and Bardo lists it the next time it opens: see [Missed posts](missed-posts.md).
