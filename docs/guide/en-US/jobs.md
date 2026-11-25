---
id: jobs
title: Jobs
group: reference
place: jobs
tour: jobs
---

# Jobs

Long work runs as a **job**: research, a script, a narration, images, clips, a render, an export, an upload, a metrics sync. Jobs run in the background, so you keep working while they go, and they survive Bardo closing. [Show me the panel](bardo:tour/jobs).

<a id="panel"></a>
## The Jobs panel

**Jobs** in the navigation opens a panel beside whatever screen you are on (over its right side, in a narrow window), and closes it again; the line beside it says how many jobs are running or waiting. The panel groups the jobs as **Running**, **Queued**, **Failed** and **Finished**, each card with what the job does and where it stands. A job that waits for its time, like a scheduled post, says when it goes.

<a id="progress"></a>
## Progress

A running job shows a bar and its percent. Screens show their own jobs too: a stage says it is generating, and its result appears when the job finishes, with no need to watch the panel.

<a id="cancel"></a>
## Cancelling

**Cancel** stops a running or queued job. What it already made is kept for a retry, and a call a provider already answered is paid and counted in [Costs](costs.md#spent). A cancelled render or export keeps the networks it finished, and a retry does the ones left.

A text Claude is still writing (a script, a scene plan, a title) is given up when its job is cancelled: Bardo stops reading and closes the connection instead of waiting for the end. Claude may charge for what it wrote until then, so [Costs](costs.md#spent) counts an estimate of the whole text, which may be more than a text cancelled early cost. A scene plan whose answer arrives just as you cancel is not applied to the project; it is paid, and **Retry** on that job, or **Plan scenes** on the Scenes stage, makes the plan from it without asking Claude again.

<a id="retry"></a>
## Retrying

When a provider fails for a passing reason (a connection that could not be made, a busy server, a rate limit), Bardo tries again by itself, up to four attempts in all, waiting longer each time, and the card says which attempt failed. When the failure is lasting, like a missing key or a refused sign-in, the job stops as **Failed**, with what went wrong and **Details**.

A paid request for text, an image, a voice or music that was sent and got no answer is not tried again by itself: the provider may have done the work and charged for it. The job stops as **Failed** and says so, and [Costs](costs.md#spent) counts an estimate of what it may have cost. See [A paid request got no answer](troubleshooting.md#answer-lost).

A clip request the provider took and that then ended without a clip (failed, cancelled or forgotten there) is not sent again by itself either, since sending it again is a new request. Its scene shows the failure, the job stops as **Failed** once the other scenes are done, and **Retry** is what sends it again ([Clips](clips.md)).

**Retry**, on a failed or cancelled job, queues it again. It goes on from the last point it saved rather than from the start. Fix what the failure names first: see [Troubleshooting](troubleshooting.md).

A scene plan or image job is not queued again while another scene job of its project is queued, running or still stopping: the two would pay for the same work. And an older scene plan job is not queued again once the project's scenes were planned or drawn after it, since it would replace that plan and its images: plan again from the [Scenes](scenes.md) stage, which asks first.

<a id="restart"></a>
## Closing Bardo mid-job

Closing Bardo, or the computer, never loses a job. The next time Bardo opens, each running job picks up from the last point it saved, and the attempt it was on does not count against its retries. A job that had already handed work to a provider (a clip being made, an upload being processed) asks the provider how it went rather than paying for it twice. A job that was waiting for the answer to a paid request for text, an image, a voice or music, which cannot be asked about, does not send it again: it stops as **Failed**, as for [a paid request that got no answer](troubleshooting.md#answer-lost), and [Costs](costs.md#spent) counts an estimate of what it may have cost. A job waiting to retry, or waiting for its time, keeps waiting.

<a id="test"></a>
## Test jobs

**Start test job** runs a ten-second countdown that makes nothing and costs nothing, to try progress, cancelling, retrying and closing Bardo mid-job. **Start failing test job** fails on purpose, so you can see an automatic retry and a failure.
