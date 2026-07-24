# scheduler

**Product 2 — not started.** See ADR-001 in `docs/Axioma_implementation_v3.md` §2.5.

Governed Campaign job queue: per-project concurrency limits, quotas, cost ceilings, retry/back-off,
cancellation (NFR-PERF-05). An L4 autonomy loop must never be able to launch an unbounded or
unbudgeted Campaign.
