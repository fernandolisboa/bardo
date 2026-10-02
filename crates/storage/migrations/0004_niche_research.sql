-- Market data per niche and market: the niche research cache (ADR-0004).
CREATE TABLE niche_research (
    id            INTEGER PRIMARY KEY,
    profile_id    TEXT NOT NULL REFERENCES user_profile (id) ON DELETE CASCADE,
    -- Niche::key, the cache identity; `niche` keeps the label as typed.
    niche_key     TEXT NOT NULL,
    niche         TEXT NOT NULL,
    country       TEXT NOT NULL,
    language      TEXT NOT NULL,
    -- Unix time in milliseconds.
    fetched_at    INTEGER NOT NULL,
    upload_volume INTEGER NOT NULL CHECK (upload_volume >= 0),
    UNIQUE (profile_id, niche_key, country, language)
) STRICT;

-- The sampled uploads behind a result, in the provider's order.
CREATE TABLE niche_research_upload (
    research_id         INTEGER NOT NULL REFERENCES niche_research (id) ON DELETE CASCADE,
    position            INTEGER NOT NULL,
    channel_id          TEXT NOT NULL,
    -- Unix time in milliseconds.
    published_at        INTEGER NOT NULL,
    views               INTEGER NOT NULL CHECK (views >= 0),
    -- NULL when the channel hides its subscriber count.
    channel_subscribers INTEGER CHECK (channel_subscribers >= 0),
    PRIMARY KEY (research_id, position)
) STRICT;

-- The niches each channel researched last, in order.
CREATE TABLE niche_seed (
    channel_id TEXT NOT NULL REFERENCES channel (id) ON DELETE CASCADE,
    position   INTEGER NOT NULL,
    niche      TEXT NOT NULL,
    PRIMARY KEY (channel_id, position)
) STRICT;
