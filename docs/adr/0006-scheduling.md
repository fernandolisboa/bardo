# ADR-0006: Running scheduled publications

- Status: Accepted; decision 2 amended by [ADR-0008](0008-network-connections.md)
- Date: 2026-09-30

## Context

Scheduled posts must go out at the chosen time. Secrets live in the user's Windows Credential Manager, which a Windows service running under another account cannot read. The PC must be on for anything local to run.

## Decision

1. **Platform-native scheduling first.** Where a network supports it, upload ahead of time and let the platform publish (YouTube: upload private with `status.publishAt`). The app can be closed and the PC off.
2. **In-app scheduler** for networks without native scheduling (Instagram): the job queue fires due publications while the app is open. Publications missed while closed run on next launch, after the user confirms. TikTok uploads go to the creator's inbox as drafts (ADR-0008), so TikTok posts are scheduled in the TikTok app, not by Bardo.
3. **Optional background agent, after the MVP.** The installer offers a per-user background agent: a Windows Task Scheduler task running as the user, which reads Credential Manager normally and needs no admin rights. Not a privileged Windows service. Proposed installer copy:
   - en-US: "Publish even when Bardo is closed? Installs a lightweight agent that sends scheduled posts in the background while you are signed in to Windows. You can turn it off later in Settings."
   - pt-BR: "Publicar mesmo com o Bardo fechado? Instala um agente leve que envia os posts agendados em segundo plano enquanto você estiver conectado ao Windows. Dá para desligar depois nas configurações."

## Consequences

- YouTube, the primary network, needs neither the app nor the PC at publish time.
- Instagram posts in the MVP go out only while the app is open; the UI shows this on the scheduling screen.
- The job queue and publication state must be shareable between the app and the future agent (same SQLite database, with locking).
- The in-app scheduler keeps each post's due time and its claim in SQLite with the publication: the run that publishes a post claims it in one statement that also checks it is due, so the agent can read and claim due posts from the same database without posting one twice. The agent still needs a lease on the job itself, so the app and the agent never run the same job at once.
- The agent (issue #87) is `bardo --agent`. Until an installer exists, Bardo registers its task from Settings › Publishing, with the copy above (the off switch removes the task), and re-registers it on open while it is on. Its task starts it at sign-in and every 15 minutes after, never as a second copy. The app and the agent each lease a job in SQLite before running it and renew the lease while it runs; a due post's claim also requires its job's lease, so the two never post one twice. While the app is open, the agent starts nothing new.
- A post whose due time passed while the app was closed is never sent silently: the app lists it on the next launch and the user sends it now, reschedules or cancels it.
