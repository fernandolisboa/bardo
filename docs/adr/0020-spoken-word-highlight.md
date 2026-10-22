# ADR-0020: Captions that light the spoken word, one libass event per word

- Status: Accepted
- Date: 2026-10-22
- Amends: the MVP spec's captions, which showed a whole line in one color

## Context

Captions came in three line-level styles (Clean, Boxed, Punch): a line showed whole, in one color, from its first word to its last. The look most Shorts use lights up the word being said. The narration already has word timings (ElevenLabs timestamps, or alignment for imported audio), but a caption keeps only its text, start and end, in narration-file time.

## Decision

1. **A highlight on every style, not new styles.** A cut's captions have a highlight: off, or one of five colors (yellow, green, cyan, pink, orange). With a highlight, the words not being said are white in every style (Punch's letters are otherwise yellow), so the lit word stands out.
2. **Color only.** The lit word changes color and nothing else. A scaled word widens the line, which shifts its neighbors and can re-wrap a long line in 9:16, so the text jumps; a word-by-word reveal hides words a viewer reads ahead. Placement stays as each style sets it, in 9:16 and 16:9 (ADR-0018).
3. **Lit from its start to the next word's start.** A word lights up when the narrator starts saying it and stays lit until the next word starts, so there is no unlit gap between words; the last stays lit until the line ends. Before the first word starts (a caption stretched earlier), the line shows unlit.
4. **Word times from the narration, not stored.** The cut keeps the narration's word timings in memory, read from the narration whenever the cut opens. A caption's words are those the narration speaks inside it (half the word or more, else any part). When the caption has that many words, as made or with a word mended, each keeps its spoken time; when the user rewrote it with more or fewer, the caption's time is shared out over its words by their length. A trimmed caption keeps its words' times within its new ends. Nothing about word times is saved, so no caption data changes.
5. **Over a cut, mapped through the narration.** A caption shown in two stretches (narration cut in the middle) lights in each stretch the words spoken there; a word spoken before a stretch starts lights it until the next word.
6. **One libass event per word, the same in preview and render.** A highlighted line is written to the ASS script as one event for each word's lit time, each the whole line with `\1c` color overrides around the spoken word. Only colors differ, so libass lays every event out alike. Karaoke tags (`\k`) were not used: they leave a word colored after it is spoken. The preview and the render draw the same script with the same fonts, as before.
7. **Per channel, per cut.** A channel has a highlight for its new cuts, like its caption style; a cut has its own, changed in the caption inspector as an edit that can be undone. New channels start at yellow. Channels from before start at off, as do cuts saved before and settings files made before: a rough cut nobody edited is rebuilt from its channel, so a highlight there would change cuts and outdate renders made before (the reasoning of ADR-0018 and ADR-0019).
8. **Plans without a highlight are unchanged.** A render plan writes the highlight and the word times only when the highlight is on, so a cut without one has the same fingerprint as before and its renders stay current.

## Consequences

- A highlighted line makes as many ASS events as it has words; libass renders one at a time, so the cost per frame is the same as a plain line.
- A rewritten caption's word times are estimates, by length; the user can mend words without losing timing, and can turn the highlight off per project.
- Scale or bounce effects, per-word colors and per-caption highlights are out of scope.
