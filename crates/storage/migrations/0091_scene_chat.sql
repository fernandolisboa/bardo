-- The scene chat (issue #368, docs/spec/scene-chat.md, ADR-0049): the
-- user's requests about one scene's image or clip and Claude's replies.
-- A chat belongs to its scene plan: kept while the plan is (fitting it to
-- a new narration keeps its id) and removed with it when the scenes are
-- planned again. Scene rows are written again on every save of the plan,
-- so the chat refers to the plan and the scene's position, not to the
-- scene row.
CREATE TABLE scene_chat_message (
    id       TEXT PRIMARY KEY NOT NULL,
    plan_id  TEXT NOT NULL REFERENCES scene_plan (id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    -- 'request' for the user's words, 'reply' for Claude's.
    author   TEXT NOT NULL CHECK (author IN ('request', 'reply')),
    -- What a request is about; NULL for a reply.
    target   TEXT CHECK (target IN ('image', 'clip')),
    text     TEXT NOT NULL,
    -- A reply's question or proposal, its options and what was applied
    -- of them, as JSON; NULL for a request.
    reply    TEXT,
    -- The scene chat job that answers a request or wrote a reply. No
    -- foreign key: finished jobs are cleared.
    job_id   TEXT,
    -- Unix time in milliseconds.
    at       INTEGER NOT NULL,
    CHECK ((author = 'request') = (target IS NOT NULL)),
    CHECK ((author = 'reply') = (reply IS NOT NULL))
) STRICT;

CREATE INDEX scene_chat_message_scene ON scene_chat_message (plan_id, position, at);
