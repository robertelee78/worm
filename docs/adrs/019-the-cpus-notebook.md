# ADR-019: The CPU's Notebook — LLM Depth, On Request Only

## Status
Implemented

## Date
2026-08-06

## Context

The instant round-over explainer is deterministic and truthful but
template-shaped. The owner wanted richer, more meaningful post-round
narrative — with the hard constraint that players never wait on a model:
LLM depth ON REQUEST ONLY, behind a "tell me more" button.

## Decisions

**On-request, never ambient.** The round-over overlay gains 📓 TELL ME
MORE. Clicking is explicit consent to wait (~13-30 s, honestly stated in
the progress line); nothing generates in the background, nothing blocks
the game, and a missing/slow model degrades to the instant summary with
an honest apology.

**Grounded or silent.** The prompt hands the model ONLY deterministic
measurements computed server-side from the round record and its ghost
event stream — outcome, cause, prediction rate and sample count, the top
guessers with hit rates, memory delta, and habit stats (left/right break
counts, fires) derived from the recorded inputs. The system prompt
forbids inventing any number or event; missing data means saying less.
The game's honesty contract extends to its narrator. First live output
cited 82.65%/98 samples, "right ten, left six", and 17 stored situations
— all real.

**Owner-auth worker, localhost only.** `claude -p --model haiku` needs
the owner's subscription auth, which the CGI user must not have. The
worker (`server/explain_worker.py`) is a stdlib HTTP server bound to
127.0.0.1:8791, run as the owner via a systemd user unit
(`worm-explain.service`, lingering enabled), fronted by Apache ProxyPass
at `/explain`. Nothing new listens publicly. Responses cache by round id
(instant repeats); a per-day cap (300) bounds subscription spend;
size/shape validation mirrors the collector's.

## Measured

Cold ~30 s (CLI startup dominant), warm ~13 s, cache hit instant. The
button's progress text quotes the real wait. Prompt-injection surface:
the record's free-text fields are engine-generated enum strings and
charset-limited ids only.
