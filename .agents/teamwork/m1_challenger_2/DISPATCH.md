# Task Assignment: M1 Challenger 2 - Persistence Fuzzing & Invariant Verification

## Working Directory
/home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive

## State Folder
/home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/m1_challenger_2

## Mandatory Documents
- /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/ORIGINAL_REQUEST.md
- /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/PROJECT.md
- /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/m1_worker/changes.md
- /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/m1_worker/handoff.md

## Mission
You are an adversarial verifier for Milestone 1.
Empirically test persistence schemas, roundtrips, and invariant corner cases:
1. Stress test serialization / deserialization of `.oplf` and `.opsf`:
   - Malformed / boundary JSON payloads (pitch 0, pitch 127, duration 0, duration u32::MAX, empty notes, 50,000 notes).
   - Verify serde alias compatibility (`"note"` vs `"pitch"`).
   - Test backward compatibility: load legacy projects without `Midi` clips.
2. Execute empirical test programs and document findings.
3. Deliver your verdict (`APPROVE` or `REQUEST_CHANGES`) and empirical evidence in `handoff.md`.


## 2026-11-08T01:22:26Z
You are M1 Challenger 2.
Read ORIGINAL_REQUEST.md: /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/ORIGINAL_REQUEST.md
Task assignment: /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/m1_challenger_2/DISPATCH.md
Scope: /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/PROJECT.md
Worker handoff: /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/m1_worker/handoff.md
Worker changes: /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/m1_worker/changes.md
Working directory: /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive
State folder: /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.agents/teamwork/m1_challenger_2

Empirically test persistence schemas, roundtrips, backward compatibility, and extreme boundary cases.
Write your findings, evidence, and verdict (APPROVE or REQUEST_CHANGES) in handoff.md in your state folder.
Update progress.md as you work.
When done, message me with your verdict and handoff link.
