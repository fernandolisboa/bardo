# ADR-0036: Text jobs at work on the same thing wait for each other, and a paid answer is not asked for twice

- Status: Accepted
- Date: 2026-11-25
- Issue: #309
- Builds on: ADR-0030 (a paid call is sent once) and ADR-0034 (a cancelled job gives up its request to Claude, and a scene plan's answer is kept until it is applied)
- Amends: ADR-0034 (its consequence "their stages still read as free the moment the job is cancelled" no longer holds, and the answer a version or a take prompt job paid for is no longer thrown away)

## Context

ADR-0034 made a cancelled job give its request to Claude up within moments, and closed three gaps for the scene plan: its stage stays busy until the cancelled run has returned, Retry in Jobs does not go around that, and an answer that arrives whole as the job is cancelled is kept instead of applied or lost. The other jobs that ask Claude for text were left as they were:

- A script, chapters, a thumbnail's text, a music prompt, metadata, ideas for a channel, a version, a persona look's prompt and a take prompt each asked "is the latest job of this kind queued or running?" before starting another. A cancelled job reads cancelled at once, while its handler may still be in the request it has yet to give up, or saving what that request brought: for those moments a second job could start beside it, and both were paid.
- That question looked at the latest job only. An older job queued again from Jobs was not seen by most stages, and Retry itself asked nothing: an older script job could be queued again beside a newer one at work.
- An answer that arrived whole as its job was told to stop was saved by most of these jobs. A version job and a take prompt job threw it away, paid: the next run (Retry after a cancel, or the job resuming by itself after the app closed) asked Claude again.

## Decision

1. **A text job names what it works on.** `Work` is one text of a project (its script, chapters, thumbnail, music prompt, metadata, versions), the ideas of a channel, the look of a persona or one take of a capture, read from the job's kind and payload. A version's script adapted again is work on that project's script. The jobs that draw what such a text describes share its work, as their stages already treated them: a thumbnail's background with its text, a look's drawing with its prompt (one job), a take's test, draft and final with its prompt.
2. **A job is at work until its handler has returned.** `Bardo::at_work` is true for a job that is queued or running, and for one that no longer reads so while this process still runs its handler (`JobQueue::runs_here`). `Bardo::work_busy` asks it of every job with the same `Work`, not only the latest.
3. **Every one of these stages asks that one question before it starts a job**: writing a script (or adapting a version again), chapters, a thumbnail's text or background, a music prompt, metadata, proposing or ranking ideas, making a version, drawing a look, and everything a take's page starts or changes while something is made for the take. The take's view counts a stopping job as busy, since its page reads the view, and so does the capture's test take; the other stages answer with their usual "already being made" message, which now says that a cancelled job may still be stopping.
4. **Retry in Jobs asks it too.** A job with a `Work` is not queued again while another job is at work on the same thing, itself included until its cancelled run has returned (`JobActionError::SameWork`). Scene jobs keep their own check (`ScenesBusy`, ADR-0034).
5. **An answer that arrives whole as its job is told to stop is paid for, so it is never asked for twice.** What each job does with it:

   | Job | The answer that arrives as the job is cancelled | Run again from Jobs, or asked for again from its stage |
   | --- | --- | --- |
   | Script | Saved, as before: the project's first script, or a new one waiting for review beside the current one. It replaces nothing. | Jobs: finds its script; asks nothing. The stage starts a new job, as it does after any script. |
   | Version adapted again | Saved, as before: it waits for review beside the script. | Jobs: finds its adaptation; asks nothing. |
   | Ideas for a channel | Saved, as before: they add to the channel's ideas, not yet ranked. | Jobs: ranks them; asks Claude nothing. |
   | Look of a persona | The prompt is kept on the job, as before; nothing is drawn. | Jobs: draws from the kept prompt. |
   | Chapters, thumbnail text, music prompt, metadata | Saved, as before, in place of the current one: that is what the job was started to do, and only a cancel that lands in the instant between the answer's end and the save meets it. | Jobs: finds what it saved; asks nothing. |
   | Version for another channel | Not applied, as before: the user said stop, so no project appears in the other channel behind their back. Now kept on the job. | Either way the same job goes on and makes the version from the kept adaptation: the stage finds it when the same version is asked for (same script, channel and template). |
   | Take prompt | Not applied, as before: the prompt the take has (maybe edited since) stays. Now kept on the job. | Either way the same job goes on and writes the kept prompt to the take: the take finds it when the same prompt is asked for (same control video, model and template). |

6. **The kept answer is one mechanism.** `Kept` is what the scene plan job kept in its checkpoint (ADR-0034), moved where the version and the take prompt jobs use it too. The answer is recorded in Costs and goes into the checkpoint as soon as it arrives, before anything is made of it, so a run that is cancelled, killed or closed while it copies a version's files or checks a take does not pay for it again. It stays through a run that was told to stop or could not save what it made (the take changed, storage failed), and is let go once applied, or when it turns out not to be usable (so that the next run asks Claude again instead of failing on it for good). The same covers a run told to stop because the app is closing: the job resumes by itself on the next start and goes on from the kept answer.
7. **The stage reuses a kept answer instead of paying for the same one.** When a version or a take prompt is asked for again and the latest job on that work holds the answer to the very same request, that job is queued again and its id returned; no new job, no budget question (nothing is paid), no request. A request that differs (another template, an edited script, another model) starts a new job as before.
8. **A version's adaptation is in Costs from the moment it arrives.** It is recorded on the other channel at arrival, under an id derived from its job (recording it twice changes nothing), and moves to the version's project once that project exists: the version's record is written first and the channel's taken back after, so a stop in between counts it twice for a moment rather than not at all.

## Consequences

- Between a cancel and the moment the cancelled run returns, the stage refuses to start the same work, and so does Retry. For a text that is moments (ADR-0034). For a job that is drawing an image or waiting on a take's provider it lasts as long as that call, which is not given up.
- A performed take's job (test, draft, final) is not queued again from Jobs while its take's prompt is being written or another job of the take is at work, where before only the take's page refused.
- A kept answer is found only while its job is the latest on that work and the request is the same to the letter. Any other request pays for a new answer, and the kept one stays on its job, unused.
- A version job that is cancelled and never run again leaves its adaptation in Costs on the other channel, with no project: it was paid, and no version exists to carry it.
- A cancel that lands in the instant between an answer's arrival and its save does not stop the save, as for a scene plan (ADR-0034).
- No migration: the checkpoint of a scene plan job reads as before.
