# How the CPU Learns You

The CPU is a purpose-built online-learning stack — no neural network, no
pre-training — that studies one specific human and compounds what it
learns across every round they ever play. Its foundation is an ensemble
of fourteen hand-crafted predictors (habit, frequency, and pattern
readers; wall-followers; a k-NN episodic memory that recalls situations
you've been in before; six "intent" models that simulate *why* you move —
food runs, weapon grabs, hunts — in both line-holding and weaving
styles; and a variable-order Markov "rhythm reader" over your voluntary
swerves), combined by two-horizon fixed-share weights so it tracks both
what you always do and what you started doing five choices ago. Above
the ensemble sits a class-conditional *turn book*: a Krichevsky–Trofimov
hazard model over 96 situation cells (your swerve cadence × where the
food is pulling you × whether you just ate × whether it's chasing you)
that learns *when* you turn, a specialist-scored side book that learns
*which way*, and a learned overshoot-correction prior that bends its
5-frame projection of you toward the side the food sits on. Every claim
of "I've read you" must survive an exact statistical test against a
class-aware baseline under an anytime-valid evidence budget — the
difficulty you see on screen is earned significance, never vibes — and
all of it persists per-player, so it remembers you between sessions.

It also learns about itself, and about the fight. Self-knowledge
ledgers track which of its hunting tactics have actually killed *you*
(Thompson-sampled preference between intercepts), which weapons bait
you (including deliberate exploratory mine placements), and how you
kill *it* — deaths while you were hunting it raise its escape margins
against you specifically, and a region-collapse alarm makes it evacuate
before your box closes. A drift alarm (its own sequential evidence
family) detects when you *change* your game — it latched for real when
its owner started scrambling his turn timing to fight the read — and a
between-round "ratchet" names exactly which region of situation-space
moved, via an exact minimum cut over the cells' co-movement graph. An
epistemic self-map counts the situations it has never seen you in;
nightly evolution hill-climbs its tuning against fixed benchmarks; and
a plateau detector guards the door against heavier machinery (neural
challengers earn evaluation only if the counting stack stops improving
on a *stationary* opponent — so far, the humans keep moving). Every
one of these reads is explainable in a sentence, and the post-round
notebook will quote you the exact numbers.
