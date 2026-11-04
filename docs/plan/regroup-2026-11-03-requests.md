# Regroup requests and their records

The operator's and the Director's requests of 2026-11-03 that the regroup planning pull request (PR 562) records, with the task and epic that hold each. A task named "standalone" lands its record and code in its own pull request.

| Request | Record | Epic |
|---|---|---|
| The working tier is named Worker | TSK-1287 | EPC-025 |
| Effort per coordination tier | TSK-1288 (AC-6 adds the effort per role from the model defaults) | EPC-025 |
| Only the Director runs the scheduled A to F sweep | TSK-1037 (AC-5 and AC-9) | EPC-025 |
| Coordination design folds, gaps 1 to 8 | TSK-439 AC-10, AC-13, AC-14; TSK-423 AC-1, AC-4; TSK-440 AC-7; TSK-1037; TSK-1287; TSK-1288; TSK-1095 | EPC-025; TSK-1095 in EPC-035 |
| A host browser fault is reported as not runnable | TSK-1289 | EPC-028 |
| TSK-1241's hosted coverage measurement | TSK-1290 (records the measurement behind the operator's decision to keep coverage on line pull requests; TSK-1241 AC-2 to AC-5 are withdrawn) | EPC-028 |
| Issue 451, bypass permissions as a named preset | TSK-1303 | EPC-032 |
| Issue 452, the sandbox as a floor with the retry off | TSK-1059 AC-5 | EPC-037 |
| Issue 464, a clean main sync into an epic line | TSK-1124 AC-7 | EPC-031 |
| Issue 472, a timeout for the generated gates job | TSK-1150 (standalone, complete on the line through PR 484; nothing new filed) | none |
| Issue 522, optional validated areas on records | TSK-1304 | EPC-031 |
| Issue 545, the landing line passed to gate targets | TSK-1190 AC-8 | EPC-027 |
| Issue 546, hosted target output readable through the API | TSK-1305 | EPC-028 |
| Issue 547, the cause behind "provider failed" | TSK-1125 AC-7 | EPC-031 |
| Issue 550, claim cuts from the fetched tip | TSK-416 AC-6 | EPC-031 |
| Issue 559, a landing commit rebuilt on the gated tree | TSK-1123 AC-13 | EPC-031 |
| Issue 561, a completion on a line that lags main | TSK-1124 AC-8 | EPC-031 |
| Citations: issue 481 and issue 415 | TSK-1123; TSK-1082 (standalone, complete) | EPC-031; none |
| Config audit limits under the no-hard-cap rule | TSK-1299 (the limits registry); TSK-1297 (coordination timing) and TSK-1300 (template timeouts) after it; TSK-1073 AC-12 (parser bounds, declared in the registry) | EPC-028; EPC-028; EPC-028; EPC-029 |
| One limits registry, declared once (the operator's configurable-by-default rule) | TSK-1299, retitled; limits tasks either depend on it or land first and migrate under its criteria; it absorbs TSK-1301, TSK-1278 and TSK-1258's bound as consumers | EPC-028 |
| Limits ship in 4.0.0 (Director, final edit) | TSK-1297, TSK-1299 and TSK-1300 moved from EPC-036 and EPC-038 | EPC-028 |
| The pre-push destination probe's timeout and attempts | TSK-1301 (standalone, coord/ci's pull request; the registry's first consumer); TSK-1298 was allocated and withdrawn | placed in EPC-028 for planning |
| Model and effort defaults | TSK-1292 | EPC-032 |
| A project registers its own model lines | TSK-1293 | EPC-032 |
| A project routes duties and roles and sets effort | TSK-1294 | EPC-032 |
| The harness choice of model and effort wins | TSK-1295 | EPC-032 |
| One adopter page for model choice and overrides | TSK-1296 | EPC-032 |
| Project overrides of managed instructions (task 6 and the customization addendum) | TSK-1302; the addendum's catalog fields and overrides are in TSK-1292 AC-3 and AC-4 and TSK-1294 AC-2 and AC-3 | EPC-033; EPC-032 |
| The `test_remotes` lint allows by file, host and reason | TSK-1271 (standalone, PR 551); its record at head 7df50388d does not yet name this change, which coord/ci adds in that pull request | none |
| How a landing candidate gets its exact hosted run | TSK-1188 AC-7 | EPC-028 |
| TSK-1286 lands through its own pull request | TSK-1286 (Execution contract) | EPC-028 |
| TSK-1001's open criteria | TSK-1291 (TSK-1001 stays cancelled) | EPC-027 |
