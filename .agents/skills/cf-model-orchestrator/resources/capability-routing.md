# Capability-routing contract

This resource defines who may produce and independently review each approved
task. It changes execution assignment, not the duo's independent discovery,
Claude-led design, plan approval, evidence gates, or safety boundary.

The session-level routing read is the seat section of `SKILL.md` ("Seats and
routing": the seat table, roles, primary-owned routing, review and
degradation), read once per session and rechecked explicitly when a tool,
binding, permission or selector changed. The plan's assignment line and route
readiness live in the quality contract's plan section. The sections below are
read on their trigger. Each section is the one home for its duties.

| Section | Read |
|---|---|
| [Admissible cross-lineage evidence](routing/evidence.md) | every task |
| [Route status and actual execution](routing/route-status.md) | when a route is being qualified, or a claim of scoped qualification, promotion or savings is made |
| [Design routing](routing/design.md) | when the task has product, UX, UI, interaction, or visual design work |
