use std::{cmp::Ordering, collections::BTreeSet};

use base64::Engine as _;
use serde_json::Value;

use super::{
    AccessMode, FilterOp, InsightsError, MAX_FIELDS, MAX_RECORDS, Policy, Predicate, Query,
    Selection, TypePolicy, text,
};

pub(super) fn pointer(path: &str) -> bool {
    if !path.starts_with('/') || path.len() > 512 || path.split('/').count() > 17 {
        return false;
    }
    path.split('/').skip(1).all(|part| {
        if part.is_empty() || part.chars().any(char::is_control) {
            return false;
        }
        let mut chars = part.chars();
        while let Some(c) = chars.next() {
            if c == '~' && !matches!(chars.next(), Some('0' | '1')) {
                return false;
            }
        }
        true
    })
}

fn contains(parent: &str, child: &str) -> bool {
    parent == child
        || child
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn classified(fields: &std::collections::BTreeMap<String, AccessMode>, path: &str) -> AccessMode {
    fields
        .iter()
        .filter(|(rule, _)| contains(rule, path))
        .map(|(_, mode)| *mode)
        .max()
        .unwrap_or(AccessMode::Deny)
}

impl Predicate {
    pub(super) fn validate(&self) -> Result<(), InsightsError> {
        if !pointer(&self.path)
            || !matches!(
                self.value,
                Value::String(_) | Value::Number(_) | Value::Bool(_)
            )
            || self.value.to_string().len() > 1024
        {
            return Err(InsightsError::Invalid);
        }
        if self.op != FilterOp::Eq && !self.value.is_number() {
            return Err(InsightsError::Invalid);
        }
        Ok(())
    }

    pub(super) fn matches(&self, value: &Value) -> bool {
        let Some(actual) = value.pointer(&self.path) else {
            return false;
        };
        match self.op {
            FilterOp::Eq if actual.is_number() && self.value.is_number() => {
                numeric_order(actual, &self.value) == Some(Ordering::Equal)
            }
            FilterOp::Eq => actual == &self.value,
            FilterOp::Lt => numeric_order(actual, &self.value) == Some(Ordering::Less),
            FilterOp::Gt => numeric_order(actual, &self.value) == Some(Ordering::Greater),
        }
    }
}

fn integer(value: &Value) -> Option<i128> {
    value
        .as_i64()
        .map(i128::from)
        .or_else(|| value.as_u64().map(i128::from))
}

fn numeric_order(actual: &Value, expected: &Value) -> Option<Ordering> {
    match (integer(actual), integer(expected)) {
        (Some(actual), Some(expected)) => Some(actual.cmp(&expected)),
        (Some(actual), None) => expected
            .as_f64()
            .map(|expected| integer_float_order(actual, expected)),
        (None, Some(expected)) => actual
            .as_f64()
            .map(|actual| integer_float_order(expected, actual).reverse()),
        (None, None) => actual.as_f64()?.partial_cmp(&expected.as_f64()?),
    }
}

#[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
fn integer_float_order(integer: i128, float: f64) -> Ordering {
    // JSON integers fit i64/u64, so saturating to i128 is safe even for enormous
    // floats. Compare the integral part exactly, then the signed fractional part.
    let order = integer.cmp(&(float as i128));
    if order != Ordering::Equal {
        return order;
    }
    if float.fract() > 0.0 {
        Ordering::Less
    } else if float.fract() < 0.0 {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

impl Selection {
    pub(super) fn validate(&self) -> Result<(), InsightsError> {
        if !text(&self.source, 128)
            || !text(&self.resource.type_name, 256)
            || self.resource.schema_version == 0
            || self.aggregate_type.as_ref().is_some_and(|v| !text(v, 256))
            || self.aggregate_id.as_ref().is_some_and(|v| !text(v, 256))
            || self.ids.len() > MAX_RECORDS
            || self.ids.iter().any(|v| !text(v, 256))
        {
            return Err(InsightsError::Invalid);
        }
        Ok(())
    }
}

impl Query {
    pub(super) fn normalize(&mut self) -> Result<(), InsightsError> {
        self.selection.validate()?;
        if !text(&self.purpose, 256)
            || self.fields.is_empty()
            || self.fields.len() > MAX_FIELDS
            || self.fields.iter().any(|field| !pointer(field))
            || self.filters.len() > 8
            || self.request_id.as_ref().is_some_and(|id| !text(id, 64))
        {
            return Err(InsightsError::Invalid);
        }
        for predicate in &self.filters {
            predicate.validate()?;
        }
        self.fields.sort();
        self.fields.dedup();
        self.selection.ids.sort();
        self.selection.ids.dedup();
        Ok(())
    }

    pub(super) fn fingerprint(
        &self,
        key: &ring::hmac::Key,
        scope: &super::AccessScope,
    ) -> Result<String, InsightsError> {
        let bytes = serde_json::to_vec(&(
            "rostfrei:insights:query",
            scope,
            &self.selection,
            &self.fields,
            &self.filters,
            &self.purpose,
        ))
        .map_err(|_| InsightsError::Invalid)?;
        Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(ring::hmac::sign(key, &bytes).as_ref()))
    }
}

impl Policy {
    pub(super) fn validate(&self) -> Result<(), InsightsError> {
        if self.types.len() > 128 {
            return Err(InsightsError::Capacity);
        }
        let mut keys = BTreeSet::new();
        for rule in &self.types {
            Selection {
                source: rule.source.clone(),
                resource: rule.resource.clone(),
                aggregate_type: None,
                aggregate_id: None,
                ids: vec![],
            }
            .validate()?;
            if !keys.insert(
                serde_json::to_string(&(&rule.source, &rule.resource))
                    .map_err(|_| InsightsError::Invalid)?,
            ) || rule.fields.len() > MAX_FIELDS
                || rule.fields.keys().any(|path| !pointer(path))
                || rule.conditions.len() > 16
            {
                return Err(InsightsError::Invalid);
            }
            for condition in &rule.conditions {
                condition.when.validate()?;
                if condition.fields.len() > MAX_FIELDS
                    || condition.fields.keys().any(|path| !pointer(path))
                {
                    return Err(InsightsError::Invalid);
                }
            }
        }
        Ok(())
    }

    pub(super) fn for_selection(&self, selection: &Selection) -> Option<&TypePolicy> {
        self.types
            .iter()
            .find(|rule| rule.source == selection.source && rule.resource == selection.resource)
    }
}

impl TypePolicy {
    // When a condition's operand is itself hidden, its truth must not be exposed
    // through differences in redaction. Conservatively apply the restriction.
    fn applicable(&self, condition: &super::ConditionalPolicy, value: Option<&Value>) -> bool {
        value.is_none()
            || self.record != AccessMode::RequestLess
            || classified(&self.fields, &condition.when.path) != AccessMode::RequestLess
            || self.conditions.iter().any(|other| {
                other
                    .record
                    .is_some_and(|mode| mode != AccessMode::RequestLess)
            })
            || self
                .fields
                .iter()
                .chain(self.conditions.iter().flat_map(|other| other.fields.iter()))
                .any(|(path, mode)| {
                    *mode != AccessMode::RequestLess
                        && (contains(path, &condition.when.path)
                            || contains(&condition.when.path, path))
                })
            || value.is_some_and(|value| condition.when.matches(value))
    }

    pub(super) fn record_mode(&self, value: Option<&Value>) -> AccessMode {
        self.conditions
            .iter()
            .filter(|condition| self.applicable(condition, value))
            .filter_map(|condition| condition.record)
            .fold(self.record, AccessMode::max)
    }

    pub(super) fn field_mode(&self, path: &str, value: Option<&Value>) -> AccessMode {
        let mut mode = classified(&self.fields, path).max(self.record_mode(value));
        for condition in &self.conditions {
            if self.applicable(condition, value) {
                for (rule, restriction) in &condition.fields {
                    if contains(rule, path) {
                        mode = mode.max(*restriction);
                    }
                }
            }
        }
        // Selecting a container must not circumvent a restriction on a descendant.
        for (rule, restriction) in self.fields.iter().chain(
            self.conditions
                .iter()
                .filter(|condition| self.applicable(condition, value))
                .flat_map(|condition| condition.fields.iter()),
        ) {
            if contains(path, rule) {
                mode = mode.max(*restriction);
            }
        }
        mode
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use serde_json::json;

    #[test]
    fn hidden_condition_operands_cannot_leak_through_another_field() {
        let rule: TypePolicy = serde_json::from_value(json!({
            "source":"people",
            "resource":{"kind":"aggregate","type":"user","schemaVersion":1},
            "record":"requestLess",
            "fields":{"/age":"requestLess","/name":"requestLess","/flag":"requestLess"},
            "conditions":[
                {"when":{"path":"/flag","op":"eq","value":true},"fields":{"/age":"requestOnly"}},
                {"when":{"path":"/age","op":"gt","value":30},"fields":{"/name":"requestOnly"}}
            ]
        }))
        .unwrap();
        for age in [20, 40] {
            assert_eq!(
                rule.field_mode("/name", Some(&json!({"flag":true,"age":age}))),
                AccessMode::RequestOnly,
                "redaction must not reveal hidden age {age}"
            );
        }
    }

    #[test]
    fn record_and_descendant_rules_keep_condition_operands_hidden() {
        let protected_record: TypePolicy = serde_json::from_value(json!({
            "source":"people",
            "resource":{"kind":"aggregate","type":"user","schemaVersion":1},
            "record":"requestOnly",
            "fields":{"/age":"requestLess","/name":"requestLess"},
            "conditions":[
                {"when":{"path":"/age","op":"lt","value":18},"fields":{"/name":"deny"}}
            ]
        }))
        .unwrap();
        let mut conditional_record = protected_record.clone();
        conditional_record.record = AccessMode::RequestLess;
        conditional_record.conditions.push(
            serde_json::from_value(json!({
                "when":{"path":"/flag","op":"eq","value":true},"record":"requestOnly"
            }))
            .unwrap(),
        );
        let mut protected_descendant = protected_record.clone();
        protected_descendant.record = AccessMode::RequestLess;
        protected_descendant
            .fields
            .insert("/age/detail".into(), AccessMode::RequestOnly);
        for (protection, rule) in [
            ("record", protected_record),
            ("conditional record", conditional_record),
            ("descendant", protected_descendant),
        ] {
            for age in [17, 34] {
                assert_eq!(
                    rule.field_mode("/name", Some(&json!({"age":age,"flag":true}))),
                    AccessMode::Deny,
                    "approving only name must not disclose {protection}-protected age {age}"
                );
            }
        }
    }

    #[test]
    fn parent_selection_and_escaped_pointers_preserve_descendant_restrictions() {
        let rule: TypePolicy = serde_json::from_value(json!({
            "source":"people",
            "resource":{"kind":"aggregate","type":"user","schemaVersion":1},
            "record":"requestLess",
            "fields":{"/profile":"requestLess","/profile/name":"requestOnly",
                "/profile/password":"deny","/a~1b":"requestOnly"}
        }))
        .unwrap();
        assert_eq!(rule.field_mode("/profile", None), AccessMode::Deny);
        assert_eq!(
            rule.field_mode("/profile/age", None),
            AccessMode::RequestLess
        );
        assert_eq!(rule.field_mode("/a~1b", None), AccessMode::RequestOnly);
        assert_eq!(rule.field_mode("/a/b", None), AccessMode::Deny);
        assert_eq!(rule.field_mode("/unknown", None), AccessMode::Deny);
        for path in ["", "/", "/bad~", "/bad~2", "/a//b"] {
            assert!(!pointer(path), "{path}");
        }
    }

    #[test]
    fn numeric_restrictions_do_not_round_large_integer_operands() {
        let predicate = Predicate {
            path: "/amount".into(),
            op: FilterOp::Gt,
            value: json!(9_007_199_254_740_992_u64),
        };
        assert!(predicate.matches(&json!({"amount":9_007_199_254_740_993_u64})));
        let decimal = Predicate {
            value: json!(9_007_199_254_740_992.0),
            ..predicate
        };
        assert!(decimal.matches(&json!({"amount":9_007_199_254_740_993_u64})));
    }
}
