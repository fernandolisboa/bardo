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

**Jobs** in the navigation opens a panel beside whatever screen you are on, and closes it again; the line beside it says how many jobs are running or waiting. The panel groups the jobs as **Running**, **Queued**, **Failed** and **Finished**, each card with what the job does and where it stands. A job that waits for its time, like a scheduled post, says when it goes.

<a id="progress"></a>
## Progress

A running job shows a bar and its percent. Screens show their own jobs too: a stage says it is generating, and its result appears when the job finishes, with no need to watch the panel.

<a id="cancel"></a>
## Cancelling

**Cancel** stops a running or queued job. What it already made is kept for a retry, and a call a provider already answered is paid and counted in [Costs](costs.md#spent). A cancelled render or export keeps the networks it finished, and a retry does the ones left.

<a id="retry"></a>
## Retrying

When a provider fails for a passing reason (a timeout, a busy server, a rate limit), Bardo tries again by itself, up to four attempts in all, waiting longer each time, and the card says which attempt failed. When the failure is lasting, like a missing key or a refused sign-in, the job stops as **Failed**, with what went wrong and **Details**.

**Retry**, on a failed or cancelled job, queues it again. It goes on from the last point it saved rather than from the start. Fix what the failure names first: see [Troubleshooting](troubleshooting.md).

<a id="restart"></a>
## Closing Bardo mid-job

Closing Bardo, or the computer, never loses a job. The next time Bardo opens, each running job picks up from the last point it saved, and the attempt it was on does not count against its retries. A job that had already handed work to a provider (a clip being made, an upload being processed) asks the provider how it went rather than paying for it twice. A job waiting to retry, or waiting for its time, keeps waiting.

<a id="test"></a>
## Test jobs

**Start test job** runs a ten-second countdown that makes nothing and costs nothing, to try progress, cancelling, retrying and closing Bardo mid-job. **Start failing test job** fails on purpose, so you can see an automatic retry and a failure.
