use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleCondition {
    pub field: String,
    pub op: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleAction {
    pub action: String,
    #[serde(default)]
    pub value: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationSpec {
    pub match_all: bool,
    pub conditions: Vec<RuleCondition>,
    pub actions: Vec<RuleAction>,
}

#[derive(Debug, Clone, Default)]
pub struct AutomationContext {
    pub document_type: String,
    pub vendor_name: Option<String>,
    pub currency: Option<String>,
    pub total: Option<Decimal>,
    pub extraction_quality: Option<f64>,
    pub validation_status: String,
    pub has_duplicates: bool,
    pub has_blocking_issues: bool,
    pub missing_required_fields: bool,
    pub country: String,
}

#[derive(Debug, Clone, Default)]
pub struct AutomationOutcome {
    pub labels: BTreeSet<String>,
    pub assigned_reviewer: Option<Uuid>,
    pub hold_for_review: bool,
    pub export_eligible: bool,
    pub notification: bool,
    pub webhook_requested: bool,
    pub matched_rules: Vec<Uuid>,
}

const FIELDS: &[&str] = &[
    "document_type",
    "vendor_name",
    "currency",
    "total",
    "extraction_quality",
    "validation_status",
    "has_duplicates",
    "has_blocking_issues",
    "missing_required_fields",
    "country",
];
const OPS: &[&str] = &["eq", "neq", "gt", "gte", "lt", "lte", "contains"];
const ACTIONS: &[&str] = &[
    "add_label",
    "assign_reviewer",
    "hold_for_review",
    "send_notification",
    "queue_export",
    "trigger_webhook",
];

pub fn validate_spec(spec: &AutomationSpec) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if spec.actions.is_empty() {
        errors.push("at least one action is required".into());
    }
    if spec.conditions.len() > 20 {
        errors.push("too many conditions (max 20)".into());
    }
    for (i, condition) in spec.conditions.iter().enumerate() {
        if !FIELDS.contains(&condition.field.as_str()) {
            errors.push(format!(
                "condition {} has unsupported field '{}'",
                i + 1,
                condition.field
            ));
        }
        if !OPS.contains(&condition.op.as_str()) {
            errors.push(format!(
                "condition {} has unsupported operator '{}'",
                i + 1,
                condition.op
            ));
        }
        if matches!(condition.field.as_str(), "total" | "extraction_quality")
            && condition.value.as_f64().is_none()
        {
            errors.push(format!("condition {} requires a numeric value", i + 1));
        }
    }
    for action in &spec.actions {
        if !ACTIONS.contains(&action.action.as_str()) {
            errors.push(format!("unsupported action '{}'", action.action));
            continue;
        }
        match action.action.as_str() {
            "add_label" => {
                let valid = action
                    .value
                    .as_ref()
                    .and_then(Value::as_str)
                    .map(|s| !s.trim().is_empty() && s.len() <= 60)
                    .unwrap_or(false);
                if !valid {
                    errors.push("add_label requires a label of 1-60 characters".into());
                }
            }
            "assign_reviewer" => {
                let valid = action
                    .value
                    .as_ref()
                    .and_then(Value::as_str)
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .is_some();
                if !valid {
                    errors.push("assign_reviewer requires a user UUID".into());
                }
            }
            _ => {}
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn text_field<'a>(ctx: &'a AutomationContext, field: &str) -> Option<String> {
    match field {
        "document_type" => Some(ctx.document_type.clone()),
        "vendor_name" => ctx.vendor_name.clone(),
        "currency" => ctx.currency.clone(),
        "validation_status" => Some(ctx.validation_status.clone()),
        "country" => Some(ctx.country.clone()),
        _ => None,
    }
}

fn number_field(ctx: &AutomationContext, field: &str) -> Option<Decimal> {
    match field {
        "total" => ctx.total,
        "extraction_quality" => ctx
            .extraction_quality
            .and_then(|v| Decimal::try_from(v).ok()),
        _ => None,
    }
}

fn bool_field(ctx: &AutomationContext, field: &str) -> Option<bool> {
    match field {
        "has_duplicates" => Some(ctx.has_duplicates),
        "has_blocking_issues" => Some(ctx.has_blocking_issues),
        "missing_required_fields" => Some(ctx.missing_required_fields),
        _ => None,
    }
}

fn matches_condition(condition: &RuleCondition, ctx: &AutomationContext) -> bool {
    if let Some(expected) = condition.value.as_bool() {
        let Some(actual) = bool_field(ctx, &condition.field) else {
            return false;
        };
        return match condition.op.as_str() {
            "eq" => actual == expected,
            "neq" => actual != expected,
            _ => false,
        };
    }
    if let Some(_number) = number_field(ctx, &condition.field) {
        let Some(actual) = number_field(ctx, &condition.field) else {
            return false;
        };
        let Some(expected) = condition
            .value
            .as_f64()
            .and_then(|f| Decimal::try_from(f).ok())
            .or_else(|| condition.value.as_str().and_then(|s| s.parse().ok()))
            .or_else(|| condition.value.as_i64().map(Decimal::from))
        else {
            return false;
        };
        return match condition.op.as_str() {
            "eq" => actual == expected,
            "neq" => actual != expected,
            "gt" => actual > expected,
            "gte" => actual >= expected,
            "lt" => actual < expected,
            "lte" => actual <= expected,
            _ => false,
        };
    }
    let Some(actual) = text_field(ctx, &condition.field) else {
        return false;
    };
    let expected = condition.value.as_str().unwrap_or_default();
    match condition.op.as_str() {
        "eq" => actual.eq_ignore_ascii_case(expected),
        "neq" => !actual.eq_ignore_ascii_case(expected),
        "contains" => actual.to_lowercase().contains(&expected.to_lowercase()),
        _ => false,
    }
}

/// Evaluate a rule. Actions are deterministic and never include approval or
/// external export; `queue_export` only marks an approved invoice as eligible
/// for an explicitly requested export.
pub fn evaluate_rule(
    rule_id: Uuid,
    spec: &AutomationSpec,
    ctx: &AutomationContext,
    outcome: &mut AutomationOutcome,
) -> bool {
    if validate_spec(spec).is_err() {
        return false;
    }
    let matched = if spec.match_all {
        spec.conditions.iter().all(|c| matches_condition(c, ctx))
    } else {
        spec.conditions.iter().any(|c| matches_condition(c, ctx))
    };
    if !matched {
        return false;
    }
    outcome.matched_rules.push(rule_id);
    for action in &spec.actions {
        match action.action.as_str() {
            "add_label" => {
                if let Some(label) = action.value.as_ref().and_then(Value::as_str) {
                    outcome.labels.insert(label.trim().to_string());
                }
            }
            "assign_reviewer" => {
                if let Some(id) = action
                    .value
                    .as_ref()
                    .and_then(Value::as_str)
                    .and_then(|s| Uuid::parse_str(s).ok())
                {
                    outcome.assigned_reviewer = Some(id);
                }
            }
            "hold_for_review" => outcome.hold_for_review = true,
            "send_notification" => outcome.notification = true,
            "queue_export" => outcome.export_eligible = true,
            "trigger_webhook" => outcome.webhook_requested = true,
            _ => {}
        }
    }
    true
}

pub fn evaluate_rules(
    rules: &[(Uuid, AutomationSpec)],
    ctx: &AutomationContext,
) -> AutomationOutcome {
    let mut outcome = AutomationOutcome::default();
    for (id, spec) in rules {
        evaluate_rule(*id, spec, ctx, &mut outcome);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> AutomationContext {
        AutomationContext {
            document_type: "supplier_invoice".into(),
            vendor_name: Some("Acme Supplies".into()),
            currency: Some("INR".into()),
            total: Some(Decimal::new(25000, 0)),
            extraction_quality: Some(0.92),
            validation_status: "valid".into(),
            has_duplicates: false,
            has_blocking_issues: false,
            missing_required_fields: false,
            country: "IN".into(),
        }
    }

    #[test]
    fn matches_all_conditions_and_applies_actions() {
        let id = Uuid::new_v4();
        let reviewer = Uuid::new_v4();
        let spec = AutomationSpec {
            match_all: true,
            conditions: vec![
                RuleCondition {
                    field: "currency".into(),
                    op: "eq".into(),
                    value: json!("INR"),
                },
                RuleCondition {
                    field: "total".into(),
                    op: "gt".into(),
                    value: json!(10000),
                },
            ],
            actions: vec![
                RuleAction {
                    action: "add_label".into(),
                    value: Some(json!("high-value")),
                },
                RuleAction {
                    action: "hold_for_review".into(),
                    value: None,
                },
                RuleAction {
                    action: "assign_reviewer".into(),
                    value: Some(json!(reviewer)),
                },
            ],
        };
        let outcome = evaluate_rules(&[(id, spec)], &ctx());
        assert_eq!(outcome.matched_rules, vec![id]);
        assert!(outcome.labels.contains("high-value"));
        assert!(outcome.hold_for_review);
        assert_eq!(outcome.assigned_reviewer, Some(reviewer));
    }

    #[test]
    fn rejects_unknown_actions_and_auto_approval() {
        let spec = AutomationSpec {
            match_all: true,
            conditions: vec![],
            actions: vec![RuleAction {
                action: "approve_invoice".into(),
                value: None,
            }],
        };
        assert!(validate_spec(&spec).is_err());
    }

    #[test]
    fn numeric_condition_does_not_match_text_invoice() {
        let spec = AutomationSpec {
            match_all: true,
            conditions: vec![RuleCondition {
                field: "total".into(),
                op: "gt".into(),
                value: json!(10),
            }],
            actions: vec![RuleAction {
                action: "hold_for_review".into(),
                value: None,
            }],
        };
        let mut c = ctx();
        c.total = None;
        let outcome = evaluate_rules(&[(Uuid::new_v4(), spec)], &c);
        assert!(!outcome.hold_for_review);
    }

    #[test]
    fn validation_requires_label_value() {
        let spec = AutomationSpec {
            match_all: true,
            conditions: vec![],
            actions: vec![RuleAction {
                action: "add_label".into(),
                value: Some(json!("")),
            }],
        };
        assert!(validate_spec(&spec).is_err());
    }
}
