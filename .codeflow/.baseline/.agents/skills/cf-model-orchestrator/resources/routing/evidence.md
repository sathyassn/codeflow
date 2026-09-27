## Admissible cross-lineage evidence

Output attributed to the other lineage is admissible cross-lineage evidence
only with native runtime provenance: the vendor session/thread/task id plus
model and effort labeled by their actual evidence source (`observed` when the
transport exposes actual values, otherwise `requested`). Unproven attribution
is reclassified as the author seat's own lineage and the cross-lineage half of
the work is redone.
A relay (plugin, adapter, relay subagent, or transport session) is transport,
not author. A same-lineage worker that produces work remains same-lineage; a
relay answering in the other vendor's name is evidence fabrication, and no
seat may simulate a missing vendor.

Every qualified native route (preferred plugin, official client fallback,
or schema-v2 delegate lifecycle) satisfies one five-obligation evidence contract:

1. **Launch**: verify the delegated task started through a native session
   artifact: a Codex thread forward or a lifecycle ready record reverse;
2. **Provenance**: native runtime provenance only: a native Codex thread ID
   with source-labeled model/effort forward; the lifecycle's
   session/digest/prompt binding reverse;
3. **Return**: verify the returned unit, scoped worktree diff, and cited
   evidence; a relay's idle or completion signal is evidence of neither;
4. **Failure**: a legible bounded failure (stable exit state, durable
   poison, or explicit harness error), never silent substitution or
   completion inferred from silence;
5. **Recheck**: evidence recheckable through the native surface after the
   fact: the resumable Codex thread forward, the durable state records until
   cleanup reverse.

Record model/effort as observed only when the transport exposes actual values;
otherwise label them requested, and never silently upgrade requested to
observed. Grade inferred completion explicitly as inferred.
