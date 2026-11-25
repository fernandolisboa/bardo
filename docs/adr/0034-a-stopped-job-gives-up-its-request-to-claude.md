# ADR-0034: A stopped job gives up its request to Claude, and a scene plan is asked for once

- Status: Accepted
- Date: 2026-11-25
- Issue: #295
- Builds on: ADR-0030 (a paid call is sent once, and a lost answer is recorded) and ADR-0031 (Claude's answers are streamed)
- Amends: ADR-0031 (its consequence "cancelling a plan job does not stop its request" no longer holds)

## Context

A job that asks Claude for text waited inside the text generator until the answer ended. The job's stop signal, which the queue sets when the user cancels the job or the app closes, never reached the request. Since ADR-0031 the answer is read as a stream that may run for minutes, which made four things worse:

- A cancelled job read as cancelled at once while Claude went on writing, and billing, to the end of the answer.
- The check that keeps one scene job per project looked at the latest job's state, which a cancel changes at once: a second plan could be asked for beside the first, and both were paid.
- Whichever answer arrived later replaced the project's plan, and removed the images drawn on the other.
- A plan in flight when the app closed stayed "running" in storage. The next start ran it again from the beginning: a second request, unasked, after the first was billed for what it had written (the gap ADR-0030 names).

Anthropic does not document what a closed connection does to a streamed request: whether Claude stops writing, and what of the answer is billed. Closing it is the only way a client has to say the answer is no longer wanted.

## Decision

1. **The text generator is told when to stop.** `TextGenerator::generate_until` takes a `Stop`, a question the waiting thread asks often. Jobs pass their own (`JobContext::stop`). `generate` stays, for callers nobody stops.
2. **The Claude adapter gives the answer up where it is.** `Stop` is asked before each send, during each wait to send again, while the answer's start is awaited and at every read of the stream, in steps of 100 ms. A wait for the start runs the request on a thread of its own, since the HTTP client cannot be interrupted. Giving up drops the body, and the thread that reads the connection closes it at the next byte that arrives or at the transport's bounds: a connection that says nothing cannot be closed from outside the client, and Claude sends keep-alives while it works.
3. **What a given-up request is.** Before anything was sent: `Unreachable`, billed nothing. After the request left: a lost answer (`AnswerLost`) with no count of what it used, like any stream that stops midway (ADR-0031), so Costs records what such a call usually uses. That overstates a request cancelled in its first seconds; a figure made from what had arrived would leave out the thought before the answer, which is billed and not shown. An answer that arrived whole as the stop came is returned as an answer: it is paid for, and the caller decides.
4. **A handler can tell a cancel from the other stops.** `JobContext::cancelled` is true once the user cancelled the job. A run stopped because the app closes, or because another runner took the job, is started again by itself; a cancelled one waits for the user. What a run leaves behind for the next one differs between the two.
5. **A scene plan's request is sent once, by a mark in the job's checkpoint.** Before the request leaves, the job saves `asked`; when the run comes back from it, with an answer or a failure, it saves `settled` and records what was billed. A run that finds `asked` knows the run before it never came back: it records the plan's estimate in Costs, fails once as a lost answer, and sends nothing. Only the user's retry sends a new request.
   - A run stopped by the app closing, with a failure Claude may bill, leaves `asked` and records nothing: the next start does both, once.
   - A failure that says nothing was sent is settled, so the queue's own retries still send it.
   - Refused the save of `asked`, the job sends nothing.
6. **An answer that arrives as its plan job is cancelled is kept, not applied.** The user said stop, so the project's plan is not replaced behind their back; the answer is paid for, so it goes into the job's checkpoint (`answered`), and retrying the job plans from it without asking Claude again. An answer that arrives while the app closes, with nobody having cancelled, becomes the plan as usual. A kept answer that turns out not to be a usable plan fails that retry and is let go, so the next retry asks Claude again.
7. **A scene job that is still stopping keeps the project busy.** `scenes_busy` also counts a scene job that no longer reads as running while its handler has not returned in this process, and the Scenes stage shows the same (`ScenesView::stopping`). With the request given up within moments, this lasts moments.

## Consequences

- Cancelling a text job ends its request within moments in Bardo, and at the next byte on the wire. Whether Claude stops writing there is for a real-key check to show (#233).
- A cancelled or cut request for text is in Costs as an estimate, where before it was missing.
- Closing Bardo while a scene plan is written no longer pays for a second plan unasked: the job shows a lost answer on the next start, and the user retries it.
- The other text jobs (a script, a localization, chapters, thumbnails, themes, persona looks, music prompts, metadata, take prompts) give their request up on a stop too, but still send it again by themselves after the app closed mid-answer; what was cut is now recorded. The mark of decision 5 is what #289 needs for them and for the other paid calls, each with its own checkpoint to fit it into.
- A job killed with its process between saving `asked` and sending (a window of microseconds) fails once as a lost answer although nothing left. The cost of that mistake is one click.
- When this process loses a job to another runner while its answer arrives whole, the plan is saved here and the other runner records a lost answer for the same request. It takes a process that slept past its lease.
- The plan kept by a cancelled job is used only by retrying that job. Asking for a new plan from the Scenes stage starts a new job and a new request.
