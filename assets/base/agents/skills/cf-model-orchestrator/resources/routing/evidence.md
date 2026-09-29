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

Every qualified native route meets the five-obligation evidence contract
(launch, provenance, return, failure, recheck) stated once in
[`cf-delegate`'s evidence contract](../../../cf-delegate/SKILL.md#evidence-contract-both-lanes).
