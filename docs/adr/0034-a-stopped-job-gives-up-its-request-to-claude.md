# ADR-0034: A cancelled job gives up its request to Claude, and a scene plan's answer is kept until it is applied

- Status: Accepted
- Date: 2026-11-25
- Issue: #295
- Builds on: ADR-0030 (a paid call is sent once, a lost answer is recorded, and a call in flight is noted on its job) and ADR-0031 (Claude's answers are streamed)
- Amends: ADR-0031 (its consequence "cancelling a plan job does not stop its request" no longer holds)
- Amended by: ADR-0036 (the other text jobs keep their stage busy while they stop, and a version or a take prompt keeps its paid answer)

## Context

A job that asks Claude for text waited inside the text generator until the answer ended. The job's stop signal, which the queue sets when the user cancels the job, when the app closes or when another runner takes the job, never reached the request. Since ADR-0031 the answer is read as a stream that may run for minutes, which made three things worse:

- A cancelled job read as cancelled at once while Claude went on writing, and billing, to the end of the answer.
- The check that keeps one scene job per project looked at the latest job's state, which a cancel changes at once: a second plan could be asked for beside the first, and both were paid.
- Whichever answer arrived later replaced the project's plan, and removed the images drawn on the other.

A fourth, a plan in flight when the app closed being sent again on the next start, is closed for every paid call by the note ADR-0030 item 10 puts on the job (#289). This decision is made to fit it.

Anthropic does not document what a closed connection does to a streamed request: whether Claude stops writing, and what of the answer is billed. Closing it is the only way a client has to say the answer is no longer wanted.

## Decision

1. **The text generator is told when to stop.** `TextGenerator::generate_until` takes a `Stop`, a question the waiting thread asks often. Every job that asks Claude for text passes its own (`JobContext::stop`). `generate` stays for a caller with nothing to stop it.
2. **A job's stop asks on a cancel, or when another runner took the job. Not when the app closes.** A closing app ends moments later, and the connection with it. A call that never came back stays noted on its job, and the next start fails the job once and records the estimate (ADR-0030). Were the call given up at the close, it would come back as a failure, lose its note, and be sent again by the run that resumes the job.
3. **The Claude adapter gives the answer up where it is.** `Stop` is asked before each send, during each wait to send again, while the answer's start is awaited and wherever a read of the stream would have to wait, in steps of 100 ms. What the reader already holds is still read, so an answer whose end had arrived when the stop asked is an answer (one whose last bytes are still on their way at that instant is given up); and an error status is read whole, since it says whether Claude started anything. Giving up drops the body, and the thread that reads the connection closes it at the next byte that arrives or at the transport's bounds: a connection that says nothing cannot be closed from outside the HTTP client, and Claude sends keep-alives while it works.
4. **A request that has not left is held back.** The wait for the answer to begin runs the request on a thread of its own, since the HTTP client cannot be interrupted. Every transport asks the stop as the request is about to leave (the real one when the connection first asks for the body) and sends nothing when it asks then. Whichever comes first, that question or the wait being given up, decides once whether the request left: a job cancelled while the connection is still being made sends nothing and is billed nothing.
5. **What a given-up request is.** Before anything was sent: `Unreachable`, billed nothing. After the request left: a lost answer (`AnswerLost`) with no count of what it used, like any stream that stops midway (ADR-0031), so Costs records what such a call usually uses. That may overstate a request cancelled in its first seconds; a figure made from what had arrived would leave out the thought before the answer, which is billed and not shown. An error Claude sent in the place of the answer keeps its own meaning.
6. **A handler can tell a cancel from the other stops.** `JobContext::cancelled` is true once the user cancelled the job. A run stopped for another reason is started again by itself; a cancelled one waits for the user.
7. **A scene plan's answer is kept from when it arrives until it is the project's plan.** It goes into the job's checkpoint as soon as it is paid for and recorded.
   - When the job was cancelled as the answer arrived, the plan is not applied: the user said stop, so the project's plan is not replaced behind their back. Asking for the plan again uses the kept answer without a second request: **Retry** on the job, or **Plan scenes** on the stage when it asks for the same plan (same narration, template and prompt) and that job is still the project's latest scene job.
   - When the plan could not be saved (storage failed, the process died), the job's next run applies the kept answer: by itself after a restart, or, for a job that failed, on **Retry** or on **Plan scenes** asking for the same plan.
   - A kept answer that turns out not to be a usable plan fails that attempt and is let go, so the next retry asks Claude again.
8. **A scene job that is still at work keeps the project busy.** `scenes_busy` counts every scene job of the project that is queued or running, not only the latest, and one that no longer reads as running while its handler has not returned in this process. The Scenes stage shows the latter (`ScenesView::stopping`). For a plan this lasts moments; for an image job, as long as the image being drawn.
9. **Retry in Jobs does not go around that.** The check that holds a retried scene job while scenes of its project are being made (`JobActionError::ScenesBusy`, #262) uses the same `scenes_busy`, so a scene job is not queued again while a scene job of its project is at work, itself included until its cancelled run has returned. A scene plan job that is no longer the project's latest scene job is not queued again at all (`ScenesMovedOn`): it would replace the plan made since, without the question the Scenes stage asks first. Where that plan holds paid images or clips, #262's `AsksFirst` says so before.

## Consequences

- Cancelling a text job ends its request within moments in Bardo, and at the next byte on the wire. Whether Claude stops writing there is for a real-key check to show (#233).
- A cancelled request for text is in Costs as an estimate, where before it was missing.
- Closing Bardo does not give a request up in the moment before the process ends; what happens to a call in flight then is ADR-0030's.
- The other text jobs (a script, a localization, chapters, thumbnails, themes, persona looks, music prompts, metadata, take prompts) give their request up on a cancel too. Their stages still read as free the moment the job is cancelled, and an answer that arrives whole as they are cancelled is still saved as before: with the request given up within moments both windows are short. ADR-0036 closes the first and decides the second for each job.
- When this process loses a job to another runner, its request is given up and recorded as an estimate here, and the runner that took the job finds the call's note and records it once more. It takes a process that slept past its lease, or one that could not renew its leases for as long as a lease lasts (storage out of reach): then no other runner need be there, and the job is taken up again here, finds the note and fails once.
- The answer kept by a cancelled or failed job is used only while that job is the project's latest scene job and the same plan is asked for. Otherwise planning starts a new job and a new request.
- A cancel that lands in the instant between the answer's arrival and the plan being saved does not stop the plan from being applied.
