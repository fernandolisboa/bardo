-- Network connections (#76, ADR-0008): a network account signed in through
-- the network's OAuth. Only the connection's state lives here; the tokens
-- and the app credentials live in Windows Credential Manager, never in this
-- database.
CREATE TABLE network_connection (
    account_id    TEXT PRIMARY KEY NOT NULL REFERENCES network_account (id) ON DELETE CASCADE,
    profile_id    TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    status        TEXT NOT NULL CHECK (status IN ('connected', 'reconnect_needed')),
    -- Who the tokens act as: for YouTube, the channel id and title.
    identity_id   TEXT NOT NULL,
    identity_name TEXT NOT NULL,
    -- The scopes granted, separated by spaces.
    scopes        TEXT NOT NULL,
    -- Unix time in milliseconds.
    expires_at    INTEGER NOT NULL,
    connected_at  INTEGER NOT NULL,
    refreshed_at  INTEGER
) STRICT;
