---
id: uploading
title: Uploading and scheduling
group: publishing
place: projects/publish
tour: publish
---

# Uploading and scheduling

The [Publish](bardo:go/projects/publish) stage turns each rendered file into a post: it writes the post's text for each network, then uploads it to a connected YouTube, Instagram Reels or TikTok account, or [exports](exporting.md) it for you to post by hand. Nothing leaves your computer until you confirm a review. [Show me the stage](bardo:tour/publish).

<a id="networks"></a>
## One post per network

Each [network account](network-accounts.md) of the channel is a row, with its handle, the post's title or caption, its state and its last export. Pick a row to see its post in the inspector. The figures at the top count the networks rendered and exported, say whether the metadata is written or edited, and what writing it cost.

A network needs its file from the [Render](render.md) stage first; until then its row says **Not rendered**.

<a id="metadata"></a>
## Metadata and its limits

**Write metadata** has Claude write every network's post at once, from the script, the channel and each account's [metadata defaults](network-accounts.md#metadata), with the project's metadata template. It runs as a job and its cost goes to [Costs](bardo:go/costs); **Write again** replaces every network's text, so it asks first when you have edited some.

Edit each network's post in the inspector. Each field counts against the network's limit, and a post over a limit can't be exported or uploaded until you fix it:

| Network | Title | Description or caption | Tags |
| --- | --- | --- | --- |
| YouTube | 100 characters | 5,000 characters | 500 characters in all |
| TikTok | none | 2,200 characters | as hashtags in the caption |
| Instagram Reels | none | 2,200 characters | up to 5 hashtags in the caption |
| X | none | 280 characters | as hashtags in the post |
| Kick | 100 characters | none | up to 10 |

YouTube also takes no < or > in the title or description. The account's footer and the hashtags are added as the network gets them, and the count includes them; **As posted**, under the fields, shows the result. **Save** keeps your edits and **Revert** goes back to what was saved.

<a id="disclosure"></a>
## Synthetic content

When the narrator's voice is flagged as a clone or a realistic synthetic voice, each network asks you to say so, and Bardo reminds you on every network's post:

- **YouTube**: "Altered or synthetic content", which the upload review ticks for you.
- **Instagram**: the AI info label, which the review ticks for you.
- **TikTok**: the AI-generated content label, which you turn on in the TikTok app; the review's reminder brings it up when the draft arrives.
- **X and Kick** have no label: say it in the post's text or title.

An export's metadata file says where to set the label on each network.

<a id="review"></a>
## The upload review

A connected account offers **Review upload**. The button stays off, with the reason under it, while the account is not connected, the render is running or out of date, the metadata is missing or breaks the network's limits, or the file doesn't meet the network's specs. The review then shows exactly what goes:

- the file, the account, and the title, description and tags (or the caption) as the network gets them;
- your choices: visibility and made for kids on YouTube, the cover and Also show in Feed on Instagram, the synthetic-content label;
- when it goes (see [below](#schedule));
- if the project already has a post on that network, a box to replace it in Bardo (the post itself stays on the network).

Confirming starts the upload as a job, with its progress here and in Jobs. If the render, the cut or the metadata changes while the review is open, Bardo closes it and asks you to review again. Each network's guide has the details: [YouTube](connect-youtube.md#upload), [Instagram](connect-instagram.md#upload), [TikTok](connect-tiktok.md#draft).

<a id="states"></a>
## Upload states

| State | Means |
| --- | --- |
| Waiting to upload | Reviewed; the job hasn't started yet |
| Uploading | Sending the file; **Stop** keeps what the network has, **Resume** sends the rest |
| Retrying | A try failed on the way; Bardo tries again shortly |
| Processing | The network has the file and is working on it |
| Still processing | Bardo stopped waiting for the network; **Check again** later |
| Scheduled | On YouTube, private until its publish time |
| Scheduled in Bardo | Instagram: Bardo posts it at its time, with Bardo open |
| Missed its time | Bardo was closed at that time; see [Missed posts](missed-posts.md) |
| Over the publishing limit | Queued until the network takes more posts |
| Uploaded | On the network, with its link |
| Kept private | YouTube kept it private: see [Uploads stay private until the audit](connect-youtube.md#private) |
| Draft in TikTok | In the TikTok inbox, for you to finish in the app |
| Upload stopped, Upload failed | Stopped by you, or refused; **Resume** or **Retry** when it can go on |

<a id="schedule"></a>
## Scheduling

Each network schedules in its own way:

- **YouTube** takes a publish time. In the review, choose **Schedule** and type the date and time, read in your computer's time zone: Bardo uploads the video as private and YouTube makes it public at that time by itself, even with Bardo and your computer off. Until then, **Change time** and **Cancel schedule** change it on YouTube. More in [Scheduling on YouTube](connect-youtube.md#schedule).
- **Instagram** takes no publish time from apps. With **Schedule**, Bardo sends the file ahead and publishes the Reel itself at that time, so keep Bardo open then and the computer on. If it is closed, nothing is posted and Bardo asks what to do when it opens: see [Missed posts](missed-posts.md).
- **TikTok** gets a draft in your inbox, never a post. You paste the caption, choose who can watch, and post or schedule it in the TikTok app. Bardo doesn't schedule drafts.

A time that has already passed is refused when you confirm.

<a id="export"></a>
## Export

**Export** writes one folder per chosen network with the rendered file and a metadata file to copy from, for posting by hand on any of the five networks. It is the way to post on X and Kick, and works for the others too. See [Exporting](exporting.md).

<a id="post"></a>
## The post

The **Post** section follows the network's post once it is up:

- An upload links its post by itself.
- For a post you made by hand, paste its link and select **Mark as posted**. **Change link** and **Unlink** fix a wrong one; unlinking drops its numbers in Bardo, and the post stays on the network.
- A TikTok draft shows its caption with **Copy caption**; once you post it in the app, mark it as posted with its link.

Linked posts get their numbers on [Performance](performance-metrics.md): YouTube's public numbers with a YouTube Data API key, and each connected account's own numbers.
