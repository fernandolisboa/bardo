-- A channel's account on one network: handle, metadata defaults and the
-- render preset values it overrides. No credentials: those live in Windows
-- Credential Manager once publishing arrives.
CREATE TABLE network_account (
    id                 TEXT PRIMARY KEY NOT NULL,
    profile_id         TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    channel_id         TEXT NOT NULL REFERENCES channel (id) ON DELETE CASCADE,
    network            TEXT NOT NULL,
    handle             TEXT NOT NULL,
    -- NULL follows the channel's content language.
    language           TEXT,
    description_footer TEXT NOT NULL,
    visibility         TEXT NOT NULL,
    -- Render preset overrides; NULL keeps the network's built-in value.
    aspect             TEXT,
    resolution         INTEGER,
    codec              TEXT,
    bitrate_kbps       INTEGER,
    max_duration_secs  INTEGER,
    loudness_tenths    INTEGER,
    created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- At most one account per network on a channel.
CREATE UNIQUE INDEX network_account_channel_network ON network_account (channel_id, network);

CREATE TABLE network_account_tag (
    account_id TEXT NOT NULL REFERENCES network_account (id) ON DELETE CASCADE,
    position   INTEGER NOT NULL,
    tag        TEXT NOT NULL,
    PRIMARY KEY (account_id, position)
) STRICT;
