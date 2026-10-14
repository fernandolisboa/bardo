# ADR-0010: Backup and restore

- Status: Accepted
- Date: 2026-10-14
- Amends: ADR-0006 (a restore holds the database and stops the background agent)

## Context

Until now the guide told users to copy `bardo.db` and the `projects` folder while Bardo was closed. Issue #143 brings backup and restore into the app. Bardo's data is one SQLite database (`%APPDATA%\Bardo\bardo.db`), which the app and the background agent may both have open (ADR-0006), and the project folders under `%LOCALAPPDATA%\Bardo\projects`, which can be large. Keys, app credentials and sign-ins live in Windows Credential Manager (ADR-0001, ADR-0008) and must never land in a file a user can copy around.

## Decision

1. **A backup is one `.zip`** the user saves where they choose: `bardo-backup.toml` (the format, the Bardo and schema versions that made it, when, whether it has media, and the sizes), `bardo.db`, and, when the user ticks "Include media", `projects/<project>/<file>`. The database goes in deflated, media stored as is (it is compressed already), with Zip64 for large files.
2. **The database is copied with SQLite's online backup API** from a connection of its own, in one step, so the copy is one consistent moment while the app and the agent keep writing. The copy is then cleaned (below), switched out of WAL, vacuumed (no page of deleted data survives) and must pass `PRAGMA integrity_check` before it goes in the zip.
3. **What stays out:** keys, credentials and sign-ins (they are not in the database or the folders), logs, exports (`Videos\Bardo`, already the user's own files), the editor's preview copies (`proxy-*`, `peaks-*`, made again when the editor opens) and the voice sample cache.
4. **Cleaning a copy**, when it is made and again when it is restored: no job lease, runner presence or hold survives; backup and restore jobs are dropped (a restored backup must not run the backup that made it). When restored, a scheduled post a run had claimed counts as claimed longer ago than the grace (ADR-0006, `due`): Bardo checks with the network and lists it as missed instead of resuming it, so a post sent after the backup is never sent again on its own. An upload Bardo does not schedule itself (YouTube, TikTok, an Instagram post sent now) that was waiting or sending in the backup fails as *under way in a restored backup* instead of resuming, since it may have gone after the backup was made; the user checks the network and retries. One the network is already processing is left to finish.
5. **Restore is a job with a review first.** The user picks a backup; Bardo reads its manifest and refuses a damaged one or one whose schema is newer than its own. The review says when it was made, what it holds and that everything here is replaced. It is refused while any job runs. The job takes the **database hold**, unpacks the backup beside the data (`%LOCALAPPDATA%\Bardo\restore`), checks and migrates the unpacked database, waits for the agent to leave, and Bardo restarts.
6. **The database hold** is a lease on the whole database (`runner_hold`, one row): while another runner's hold is live, a runner starts no job and the agent stops; the holder renews it as a job lease, so a crashed restore frees it by itself. The agent also refuses to start while a restore waits to be applied.
7. **The files are swapped at the next start**, before the database opens: the current database (and the projects folder, when the backup has media) moves aside, the restored one moves in, and the old one is deleted. If any move fails, everything moves back and Bardo opens the data as it was, saying so. If a file cannot move back, the earlier data stays in `restore\previous`, marked to keep: nothing deletes it, a new restore refuses to stage over it, and Settings › Backup says where it is. A start that finds the database already swapped but the media not (Bardo stopped between the two) finishes the media move. A backup without media leaves the projects folder as it is.
10. **One window holds the data.** Bardo's window takes a lock file (`%LOCALAPPDATA%\Bardo\window.lock`) at start, waiting a few seconds for a window that is restarting after a restore. A second window does not get it: it neither applies nor tidies a restore, ends no hold, and refuses to start one.
8. **After a restore** Settings › Backup lists what this computer still needs: keys to save again, app credentials to save again and accounts to reconnect, each from the live state (on the same computer, usually nothing).
9. **Automatic backups**, on by default: the first start in a week backs up the database only to `%LOCALAPPDATA%\Bardo\backups`, keeping the last four. They are listed in Settings › Backup, each with Restore.

## Consequences

- A backup made with Bardo open is safe to make at any time; the agent never has to stop for one.
- Restore needs a restart, and the restore job's progress is the only thing to wait for; the swap at start is moves, not copies.
- A backup restores into the same or a newer Bardo. A newer backup asks the user to update Bardo first.
- Keys and sign-ins never travel. Moving to another computer still means saving keys and reconnecting, which the after-restore list makes explicit.
- Automatic backups live on the same disk as the data: they protect against mistakes and bad updates, not a failed disk. The guide says to keep a manual backup elsewhere.
