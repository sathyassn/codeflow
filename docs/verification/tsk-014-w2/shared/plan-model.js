// Shared subject model for the P1 candidates.
//
// This is deliberately NOT a shared design layer — it carries no styling, no
// component, and no token. It is the derivation of the subject itself: waves,
// readiness, the longest remaining chain, and room. All three P1 candidates
// import it so that they provably encode the same facts and can be compared on
// encoding alone. `tools/verify.mjs` recomputes every value here from the
// repository frontmatter and rejects disagreement.
window.planModel = (data) => {
  const byId = new Map(data.tasks.map((t) => [t.id, t]));
  const done = (t) => t.status === "complete" || t.status === "cancelled";
  const open = data.tasks.filter((t) => !done(t));
  const landed = data.tasks.filter((t) => t.status === "complete");
  const cancelled = data.tasks.filter((t) => t.status === "cancelled");

  const streamOf = (t) => {
    if (!t.specs.length) return "release";
    const presentation = t.specs.includes("SPC-004");
    const portal = t.specs.includes("SPC-005");
    if (presentation && portal) return "both utilities";
    if (presentation) return "presentation";
    if (portal) return "portal";
    return "method";
  };

  const waveCache = new Map();
  const wave = (id) => {
    if (waveCache.has(id)) return waveCache.get(id);
    const blockers = byId.get(id).deps.map((d) => byId.get(d)).filter((d) => d && !done(d));
    const value = blockers.length ? 1 + Math.max(...blockers.map((b) => wave(b.id))) : 1;
    waveCache.set(id, value);
    return value;
  };
  open.forEach((t) => wave(t.id));
  const maxWave = Math.max(...open.map((t) => wave(t.id)));

  const successors = (id) => open.filter((t) => t.deps.includes(id));
  const longestCache = new Map();
  const longestToSink = (id) => {
    if (longestCache.has(id)) return longestCache.get(id);
    const next = successors(id);
    const value = next.length ? 1 + Math.max(...next.map((s) => longestToSink(s.id))) : 0;
    longestCache.set(id, value);
    return value;
  };
  open.forEach((t) => longestToSink(t.id));
  const latest = (id) => maxWave - longestToSink(id);
  const slack = (id) => latest(id) - wave(id);

  // The longest remaining path to the sink: start at the task furthest from it,
  // then always step to the successor that is itself furthest from it.
  const head = open.reduce((best, t) => (longestToSink(t.id) > longestToSink(best.id) ? t : best), open[0]);
  const chain = [head.id];
  let cursor = head.id;
  while (successors(cursor).length) {
    const next = successors(cursor).reduce((best, s) => (longestToSink(s.id) > longestToSink(best.id) ? s : best));
    chain.push(next.id);
    cursor = next.id;
  }

  const startable = open.filter((t) => t.deps.every((d) => done(byId.get(d)))).map((t) => t.id);
  const room = Object.fromEntries(open.filter((t) => slack(t.id) > 0).map((t) => [t.id, slack(t.id)]));

  return {
    data, byId, done, open, landed, cancelled, streamOf,
    wave, slack, latest, longestToSink, successors, maxWave, chain, startable, room,
    // Published to the DOM by every P1 candidate so the render harness can
    // capture it and verify.mjs can compare all three against the repository.
    answer: { startable, chain, room },
  };
};
