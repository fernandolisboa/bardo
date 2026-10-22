-- Captions that light the spoken word (#181): each cut's highlight and
-- each channel's for new cuts. Codes are checked by the domain, as the
-- caption styles are.

-- Cuts and channels from before keep their captions as they were: a rough
-- cut nobody edited is rebuilt from its channel, so a highlight there
-- would change cuts and outdate renders made before. New channels get the
-- domain's default highlight.
ALTER TABLE timeline ADD COLUMN caption_highlight TEXT NOT NULL DEFAULT 'off';
ALTER TABLE channel ADD COLUMN caption_highlight TEXT NOT NULL DEFAULT 'off';
