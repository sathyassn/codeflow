use std::collections::BTreeMap;
use std::path::Path;

use super::{Alternative, Catalog, Effort, OperatorOverride, Target};

/// Parse the invocation's exact seat/line, harness and effort.
///
/// # Errors
/// Rejects unknown routes, missing harnesses and unsupported effort spellings.
pub fn parse_override_route(
    catalog: &Catalog,
    route: &str,
    effort: &str,
) -> Result<Alternative, String> {
    let (id, harness) = route
        .split_once('@')
        .ok_or("override route must be seat-or-line@harness")?;
    let target = if catalog.seat(id).is_some() {
        Target::Seat(id.into())
    } else if catalog.line(id).is_some() {
        Target::Line(id.into())
    } else {
        return Err("override route names unknown seat or line".into());
    };
    if !catalog
        .families
        .iter()
        .any(|family| family.harnesses.iter().any(|h| h == harness))
    {
        return Err("override route names unsupported harness".into());
    }
    let effort: Effort = serde_json::from_value(serde_json::Value::String(effort.into()))
        .map_err(|e| e.to_string())?;
    Ok(Alternative {
        target,
        harness: Some(harness.into()),
        effort,
    })
}

/// Read exactly one override from the task's anchored Execution contract.
/// Approval of the planning record is authority; this does not authenticate
/// the operator independently.
///
/// # Errors
/// Rejects absent, duplicate, incomplete or mismatched override records.
pub fn anchored_override(
    root: &Path,
    catalog: &Catalog,
    task: &str,
    override_id: &str,
    duty: &str,
    route: &Alternative,
) -> Result<OperatorOverride, String> {
    if task != override_id {
        return Err("OPERATOR_OVERRIDE task mismatch".into());
    }
    if duty != "design" {
        return Err("OPERATOR_OVERRIDE is only valid for design".into());
    }
    let content = crate::workgraph::work_start::anchored_task_content(root, task)?;
    if content.matches("\n## Execution contract\n").count() != 1 {
        return Err("task requires exactly one Execution contract".into());
    }
    let contract = content
        .split_once("\n## Execution contract\n")
        .ok_or("missing Execution contract")?
        .1;
    let contract = contract.split("\n## ").next().unwrap_or(contract);
    let lines: Vec<_> = contract.lines().map(str::trim).collect();
    let starts: Vec<_> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| **line == "OPERATOR_OVERRIDE")
        .map(|(i, _)| i)
        .collect();
    if starts.len() != 1 {
        return Err("Execution contract requires exactly one OPERATOR_OVERRIDE block".into());
    }
    let mut fields = BTreeMap::new();
    for line in &lines[starts[0] + 1..] {
        if line.is_empty() || line.starts_with("```") {
            break;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or("invalid OPERATOR_OVERRIDE field")?;
        if !["task", "duty", "route", "effort", "plan", "instruction"].contains(&key)
            || fields.insert(key, value.trim()).is_some()
        {
            return Err("unknown or duplicate OPERATOR_OVERRIDE field".into());
        }
    }
    let field = |name| {
        fields
            .get(name)
            .copied()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("missing OPERATOR_OVERRIDE {name}"))
    };
    let plan = field("plan")?;
    let version = plan
        .strip_prefix("Plan v")
        .ok_or("override plan must name Plan vN")?;
    if version.is_empty()
        || !version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("override plan must name Plan vN".into());
    }
    let record = OperatorOverride {
        task: field("task")?.into(),
        duty: field("duty")?.into(),
        route: parse_override_route(catalog, field("route")?, field("effort")?)?,
        plan_version: plan.into(),
        instruction_record: field("instruction")?.into(),
    };
    if record.task != task {
        return Err("OPERATOR_OVERRIDE task mismatch".into());
    }
    if record.duty != duty {
        return Err("OPERATOR_OVERRIDE duty mismatch".into());
    }
    if record.route.target != route.target || record.route.harness != route.harness {
        return Err("OPERATOR_OVERRIDE route mismatch".into());
    }
    if record.route.effort != route.effort {
        return Err("OPERATOR_OVERRIDE effort mismatch".into());
    }
    Ok(record)
}
