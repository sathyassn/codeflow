// Package claim manages resource claims for coordination between agents.
// Claims provide exclusive or shared access to file patterns with TTL-based
// expiration. The claim state is persisted as a JSON coordination document
// at .state/coordination/state.json, with claim lifecycle events appended
// to the JSONL ledger for rebuild authority.
package claim
