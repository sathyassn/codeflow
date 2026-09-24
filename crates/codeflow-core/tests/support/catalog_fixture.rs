use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn version(id: &str, harnesses: &[&str], seat: Option<&str>) -> Value {
    json!({
        "id": id, "alias": format!("{id}-alias"), "pinned_id": format!("{id}-pin"),
        "selectors": harnesses.iter().map(|h| ((*h).to_owned(), format!("{id}-pin"))).collect::<BTreeMap<_,_>>(),
        "efforts": ["medium", "high", "xhigh"], "lifecycle": "active",
        "designations": seat.map(|s| vec![json!({"seat":s,"date":"2026-09-23","record":"records/designation.md"})]).unwrap_or_default(),
        "qualification": []
    })
}

fn alt(kind: &str, id: &str, effort: &str, harness: Option<&str>) -> Value {
    json!({"target":{"kind":kind,"id":id},"harness":harness,"effort":effort})
}

fn participant(id: &str, alternatives: Vec<Value>, relation: &str) -> Value {
    json!({"id":id,"alternatives":Value::Array(alternatives),"relation":relation,"label":"required"})
}

fn duty(participants: Vec<Value>) -> Value {
    json!({"required":Value::Array(participants),"triggered":[]})
}

pub fn fixture() -> Value {
    let seat = |id| alt("seat", id, "high", None);
    let worker = |id| alt("line", id, "medium", None);
    let independent = || {
        participant(
            "independent",
            vec![seat("quartz-seat"), seat("orchid-seat")],
            "opposite-author",
        )
    };
    let mut second = participant(
        "advisory",
        vec![alt("line", "orchid-support", "high", None)],
        "same-author",
    );
    second["label"] = json!("second-opinion");
    json!({
        "schema_version":5,"policy_id":"fictional-pair",
        "families":[
            {"id":"orchid","provider":"anthropic","lineage":"claude","harnesses":["claude-code"],"probes":["claude-cli-version"],"usage_bucket":"orchid-bucket"},
            {"id":"quartz","provider":"openai","lineage":"codex","harnesses":["codex-cli","codex-app"],"probes":["codex-cli-version"],"usage_bucket":"quartz-bucket"},
            {"id":"cinder","provider":"xai","lineage":"grok","harnesses":["grok-cli"],"probes":["grok-cli-version"],"usage_bucket":"cinder-bucket"}
        ],
        "lines":[
            {"id":"orchid-main","family":"orchid","adoption":"manual","adopted_version":"orchid-one","versions":[version("orchid-one",&["claude-code"],Some("orchid-seat"))]},
            {"id":"orchid-support","family":"orchid","adoption":"manual","adopted_version":"orchid-two","versions":[version("orchid-two",&["claude-code"],Some("orchid-seat"))]},
            {"id":"quartz-main","family":"quartz","adoption":"manual","adopted_version":"quartz-one","versions":[version("quartz-one",&["codex-cli","codex-app"],Some("quartz-seat"))]},
            {"id":"quartz-worker","family":"quartz","adoption":"workers","adopted_version":"quartz-two","versions":[version("quartz-two",&["codex-cli","codex-app"],None)]},
            {"id":"cinder-main","family":"cinder","adoption":"manual","adopted_version":"cinder-one","versions":[version("cinder-one",&["grok-cli"],Some("cinder-seat"))]}
        ],
        "seats":[
            {"id":"orchid-seat","family":"orchid","role":"claude-judgment-primary","lines":["orchid-main","orchid-support"]},
            {"id":"quartz-seat","family":"quartz","role":"codex-engineering-primary","lines":["quartz-main"]},
            {"id":"cinder-seat","family":"cinder","role":"grok-engineering-primary","lines":["cinder-main"]}
        ],
        "standing_seats":["orchid-seat","quartz-seat"],"design_owner":"orchid-seat",
        "duties":{
            "orchestrate":duty(vec![participant("host",vec![json!({"target":{"kind":"host-seat"},"harness":null,"effort":"high"})],"any")]),
            "independent-plan":duty(vec![participant("design-primary",vec![seat("orchid-seat")],"any"),participant("engineering-primary",vec![seat("quartz-seat")],"any")]),
            "body-review":duty(vec![participant("engineering-primary",vec![seat("quartz-seat")],"any"),participant("design-primary",vec![seat("orchid-seat")],"any")]),
            "design":duty(vec![participant("designer",vec![seat("orchid-seat")],"any")]),
            "unit-review":{"required":[independent()],"triggered":[{"trigger":"extra-review","unless_author":"grok","participant":participant("extra",vec![seat("cinder-seat")],"opposite-author")}]},
            "bounded-execution":duty(vec![participant("executor",vec![worker("quartz-worker"),worker("orchid-main")],"any")]),
            "light-execution":duty(vec![participant("executor",vec![worker("quartz-worker")],"any")]),
            "engineering-implementation":duty(vec![participant("executor",vec![seat("quartz-seat"),worker("quartz-worker"),worker("orchid-main")],"any")]),
            "evidence-collection":duty(vec![participant("collector",vec![worker("orchid-main"),worker("quartz-worker")],"any")]),
            "design-implementation":duty(vec![participant("worker",vec![worker("orchid-support")],"any")]),
            "reasoning-support":duty(vec![participant("reasoner",vec![alt("line","orchid-main","high",None),alt("line","orchid-support","high",None),alt("line","quartz-worker","high",None)],"same-host")]),
            "general-review":duty(vec![participant("independent",vec![seat("cinder-seat"),seat("orchid-seat")],"opposite-author"),second]),
            "design-and-editorial-review":duty(vec![participant("advisory",vec![alt("line","orchid-support","high",None)],"any"),participant("executability",vec![seat("quartz-seat")],"any")]),
            "consultation":duty(vec![participant("consultant",vec![alt("line","orchid-support","high",None),seat("quartz-seat"),seat("cinder-seat")],"any")]),
            "computer-use-qa":duty(vec![participant("qa",vec![alt("seat","quartz-seat","high",Some("codex-app")),alt("seat","orchid-seat","high",Some("claude-code"))],"opposite-author")])
        },
        "high_triggers":["material"],"xhigh_triggers":["deep"],"named_policies":{"extra-review":["material"]},"rules":["Launch the resolved pinned identity."],"bindings":[]
    })
}
