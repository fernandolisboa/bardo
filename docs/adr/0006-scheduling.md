# ADR-0006: Scheduling scheduled publications

- Status: Accepted
- Date: 2026-09-30

## Context

Scheduled posts must go out at the chosen time. Secrets live in the user's Windows Credential Manager, which a Windows service running under another account cannot read. The PC must be on for anything local to run.

## Decision

1. **Platform-native scheduling first.** Where a network supports it, upload ahead of time and let the platform publish (YouTube: upload private with `status.publishAt`). The app can be closed and the PC off.
2. **In-app scheduler** for networks without native scheduling (TikTok, Instagram): the job queue fires due publications while the app is open. Publications missed while closed run on next launch, after the user confirms.
3. **Optional background agent, after the MVP.** The installer offers a per-user background agent: a Windows Task Scheduler task running as the user, which reads Credential Manager normally and needs no admin rights. Not a privileged Windows service. Proposed installer copy:
   - en-US: "Publish even when Bardo is closed? Installs a lightweight agent that sends scheduled posts in the background while you are signed in to Windows. You can turn it off later in Settings."
   - pt-BR: "Publicar mesmo com o Bardo fechado? Instala um agente leve que envia os posts agendados em segundo plano enquanto você estiver conectado ao Windows. Dá para desligar depois nas configurações."

## Consequences

- YouTube, the primary network, needs neither the app nor the PC at publish time.
- TikTok and Instagram posts in the MVP go out only while the app is open; the UI shows this on the scheduling screen.
- The job queue and publication state must be shareable between the app and the future agent (same SQLite database, with locking).
