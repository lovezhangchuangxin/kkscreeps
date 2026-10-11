//! 对已提交快照计算断言，整数阈值与差值保留精度。
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Comparison {
    Eq,
    Gte,
    DeltaGte,
    Exists,
    Missing,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Assertion {
    pub(super) pointer: String,
    pub(super) op: Comparison,
    #[serde(default)]
    pub(super) value: Value,
}

pub(super) fn evaluate(assertion: &Assertion, initial: &Value, final_state: &Value) -> Value {
    let actual = final_state.pointer(&assertion.pointer);
    let before = initial.pointer(&assertion.pointer);
    let delta = actual.zip(before).and_then(|(a, b)| difference(a, b));
    let passed = match assertion.op {
        Comparison::Eq => actual == Some(&assertion.value),
        Comparison::Gte => actual.is_some_and(|a| at_least(a, &assertion.value)),
        Comparison::DeltaGte => actual.zip(before).is_some_and(|(a, b)| {
            match (integer(a), integer(b), integer(&assertion.value)) {
                (Some(a), Some(b), Some(expected)) => a - b >= expected,
                _ => delta
                    .as_ref()
                    .is_some_and(|d| at_least(d, &assertion.value)),
            }
        }),
        Comparison::Exists => actual.is_some_and(|v| !v.is_null()),
        Comparison::Missing => actual.is_none_or(Value::is_null),
    };
    json!({"pointer": assertion.pointer, "op": assertion.op, "expected": assertion.value,
        "before": before, "actual": actual, "delta": delta, "passed": passed})
}

fn integer(value: &Value) -> Option<i128> {
    value
        .as_i64()
        .map(i128::from)
        .or_else(|| value.as_u64().map(i128::from))
}

/// JSON 整数保留精度；混合小数比较拒绝将过大的整数舍入为同一 f64。
fn safe_float(value: &Value) -> Option<f64> {
    if integer(value).is_some_and(|n| n.abs() > 9_007_199_254_740_991) {
        return None;
    }
    value.as_f64().filter(|n| n.is_finite())
}

fn at_least(actual: &Value, expected: &Value) -> bool {
    match (integer(actual), integer(expected)) {
        (Some(a), Some(b)) => a >= b,
        _ => safe_float(actual)
            .zip(safe_float(expected))
            .is_some_and(|(a, b)| a >= b),
    }
}

fn difference(actual: &Value, before: &Value) -> Option<Value> {
    if let (Some(a), Some(b)) = (integer(actual), integer(before)) {
        let delta = a - b;
        return Some(if let Ok(n) = i64::try_from(delta) {
            json!(n)
        } else if let Ok(n) = u64::try_from(delta) {
            json!(n)
        } else {
            json!(delta.to_string())
        });
    }
    safe_float(actual)
        .zip(safe_float(before))
        .map(|(a, b)| json!(a - b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_integer_thresholds_do_not_round_into_false_passes() {
        let final_state = json!({"points": 9_007_199_254_740_992_u64});
        let initial = json!({"points": 9_007_199_254_740_991_u64});
        let assertion = Assertion {
            pointer: "/points".into(),
            op: Comparison::Gte,
            value: json!(9_007_199_254_740_993_u64),
        };
        assert_eq!(
            evaluate(&assertion, &initial, &final_state)["passed"],
            false
        );
        let assertion = Assertion {
            op: Comparison::DeltaGte,
            value: json!(2),
            ..assertion
        };
        assert_eq!(
            evaluate(&assertion, &initial, &final_state)["passed"],
            false
        );
        let assertion = Assertion {
            value: json!(1),
            ..assertion
        };
        assert_eq!(evaluate(&assertion, &initial, &final_state)["passed"], true);
        assert_eq!(evaluate(&assertion, &initial, &final_state)["delta"], 1);
    }

    #[test]
    fn escaped_aliases_and_missing_numeric_values_are_distinct() {
        let state = json!({"objects": {"a/b~c": null}, "n": "1"});
        let assertion = Assertion {
            pointer: "/objects/a~1b~0c".into(),
            op: Comparison::Eq,
            value: Value::Null,
        };
        assert_eq!(evaluate(&assertion, &state, &state)["passed"], true);
        let assertion = Assertion {
            pointer: "/objects/missing".into(),
            ..assertion
        };
        assert_eq!(evaluate(&assertion, &state, &state)["passed"], false);
        let assertion = Assertion {
            pointer: "/n".into(),
            op: Comparison::Gte,
            value: json!(0),
        };
        assert_eq!(evaluate(&assertion, &state, &state)["passed"], false);
    }
}
