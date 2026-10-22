---
id: uploading
title: Uploading and scheduling
group: publishing
place: projects/publish
tour: publish
---

# Uploading and scheduling

The [Publish](bardo:go/projects/publish) stage turns each rendered file into a post: it writes the post's text for each network, then uploads it to a connected YouTube, Instagram Reels, TikTok or X account, or [exports](exporting.md) it for you to post by hand. Nothing leaves your computer until you confirm a review. [Show me the stage](bardo:tour/publish).

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

<a id="thumbnail"></a>
## Thumbnail

A long video's YouTube post has a **Thumbnail** card in the inspector: the picture YouTube shows before the video plays, 1280 × 720. A Short has none, because YouTube takes a Short's thumbnail from the video.

- **Write with Claude** writes the thumbnail's text (a short hook that adds to the title) and a prompt for its background, from the Thumbnail [template](templates.md). The button shows what the call costs.
- **Text** is drawn in big letters, up to 40 characters; leave it empty for the picture alone. Pick its **Text position** (top or bottom, left, middle or right) and its **Look**: Bold (white with a black outline), Yellow (yellow with a black outline) or Banner (white on a dark band). Each channel has a default look in its [form](channels.md#defaults).
- **Background** is a strip of pictures: the project's scene images, free, and every background drawn so far. **Draw a background** has the channel's image model draw one from the **Background prompt**, after showing its cost; drawing again adds to the strip and never replaces the picture in use.
- **Save thumbnail** makes the picture, off to the side so Bardo stays responsive, and shows it. Changes after that mark it **Changed since it was saved** until you save again.

The saved picture goes with the YouTube upload (see [the review](#review)) and into the YouTube folder of an [export](exporting.md#folder).

<a id="chapters"></a>
## Chapters

The YouTube card of a long video also has **Chapters**: named parts of YouTube's progress bar, from timestamps Bardo adds to the description. **Write chapters** has Claude split the narration into chapters from the Chapters template, each starting at a sentence, with a short title.

Each chapter shows its time in the cut as it is now (`m:ss`, or `h:mm:ss` from an hour). Rename one, remove it, or start it a sentence earlier or later; the first always starts at 0:00. Chapters follow the narration, so trimming the cut in the editor moves their times with it.

**Add chapters to the description** puts them between the description and the account's footer, and they count toward YouTube's 5,000 characters; when they would push the description past that, or a title holds a < or >, they stay out with a warning. YouTube shows chapters only when there are at least three, each at least 10 seconds long, the first at 0:00. Chapters that break a rule, or start in a part the cut removed, are left out of the description with a warning that says why, so they never stop an upload or export. A narration made after the chapters were written asks you to write them again.

<a id="disclosure"></a>
## Synthetic content

When the narrator's voice is flagged as a clone or a realistic synthetic voice, each network asks you to say so, and Bardo reminds you on every network's post:

- **YouTube**: "Altered or synthetic content", which the upload review ticks for you.
- **Instagram**: the AI info label, which the review ticks for you.
- **TikTok**: the AI-generated content label, which you turn on in the TikTok app; the review's reminder brings it up when the draft arrives.
- **X**: the Made with AI label, which the upload review ticks for you. Posting an export by hand, say it in the post's text.
- **Kick** has no label: say it in the title.

An export's metadata file says where to set the label on each network.

<a id="review"></a>
## The upload review

A connected account offers **Review upload**. The button stays off, with the reason under it, while the account is not connected, the render is running or out of date, the metadata is missing or breaks the network's limits, or the file doesn't meet the network's specs. The review then shows exactly what goes:

- the file, the account, and the title, description and tags (or the caption) as the network gets them;
- on YouTube, the saved [thumbnail](#thumbnail) with **Send this thumbnail**, ticked;
- your choices: visibility and made for kids on YouTube, the cover and Also show in Feed on Instagram, the synthetic-content label;
- on X, what X bills for the post, and a question first when it would reach X's budget;
- when it goes (see [below](#schedule));
- if the project already has a post on that network, a box to replace it in Bardo (the post itself stays on the network).

Confirming starts the upload as a job, with its progress here and in Jobs. YouTube sets the thumbnail once it has the video; if it refuses, the video stays up and the post says why. Most often the channel is not verified for custom thumbnails: verify it at youtube.com/verify. Either way you can set the picture in YouTube Studio from the exported file. If the render, the cut, the metadata or the thumbnail changes while the review is open, Bardo closes it and asks you to review again. Each network's guide has the details: [YouTube](connect-youtube.md#upload), [Instagram](connect-instagram.md#upload), [TikTok](connect-tiktok.md#draft), [X](connect-x.md#upload).

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
| Scheduled in Bardo | Instagram and X: Bardo posts it at its time, with Bardo open or the [background agent](background-agent.md) on |
| Missed its time | Bardo was closed at that time; see [Missed posts](missed-posts.md) |
| Over the publishing limit | Queued until the network takes more posts |
| Uploaded | On the network, with its link |
| Kept private | YouTube kept it private although you asked for more: see [the possible causes](platform-audits.md#youtube-private) |
| Draft in TikTok | In the TikTok inbox, for you to finish in the app |
| Upload stopped, Upload failed | Stopped by you, or refused; **Resume** or **Retry** when it can go on |

<a id="schedule"></a>
## Scheduling

Each network schedules in its own way:

- **YouTube** takes a publish time. In the review, choose **Schedule** and type the date and time, read in your computer's time zone: Bardo uploads the video as private and YouTube makes it public at that time by itself, even with Bardo and your computer off. Until then, **Change time** and **Cancel schedule** change it on YouTube. More in [Scheduling on YouTube](connect-youtube.md#schedule).
- **Instagram** takes no publish time from apps. With **Schedule**, Bardo sends the file ahead and publishes the Reel itself at that time, so keep Bardo open then and the computer on, or turn on the [background agent](background-agent.md), which sends it with Bardo closed. If neither runs, nothing is posted and Bardo asks what to do when it opens: see [Missed posts](missed-posts.md).
- **X** takes no publish time from apps either: it schedules like Instagram, with Bardo making the post at its time. See [Scheduling a post](connect-x.md#schedule).
- **TikTok** gets a draft in your inbox, never a post. You paste the caption, choose who can watch, and post or schedule it in the TikTok app. Bardo doesn't schedule drafts.

A time that has already passed is refused when you confirm.

<a id="export"></a>
## Export

**Export** writes one folder per chosen network with the rendered file and a metadata file to copy from, for posting by hand on any of the five networks. It is the way to post on Kick, and works for the others too. See [Exporting](exporting.md).

<a id="post"></a>
## The post

The **Post** section follows the network's post once it is up:

- An upload links its post by itself.
- For a post you made by hand, paste its link and select **Mark as posted**. **Change link** and **Unlink** fix a wrong one; unlinking drops its numbers in Bardo, and the post stays on the network.
- A TikTok draft shows its caption with **Copy caption**; once you post it in the app, mark it as posted with its link.

Linked posts get their numbers on [Performance](performance-metrics.md): YouTube's public numbers with a YouTube Data API key, and each connected account's own numbers.
