# JEV (TypeSafe System One) fixtures

Raw HTTP responses (status line, headers, body) that the decision engine
tests replay instead of calling `POST /v1/systemone`. Noise headers
(cookies, dates, request ids) were dropped.

Recorded on 2026-10-02 from the live API with an invalid key:

- `systemone-rejected.http`.

Written from the documented response shapes (docs.typesafe.ai, API
reference), because recording them needs a real key (re-record when
ranking misbehaves with a real key):

- `systemone-answers.http`: one score, one choice and one yes/no (`noul`)
  answer.
- `systemone-rate-limited.http` (429 with `retry-after`),
  `systemone-overloaded.http` (529), `systemone-invalid.http` (422).

Synthetic, for edge cases: `systemone-missing-answer.http` (a question
left unanswered).
