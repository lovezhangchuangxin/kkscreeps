use std::collections::BTreeMap;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{FreezeError, PlanHash};

pub(crate) fn canonical_hash(value: &impl Serialize) -> Result<PlanHash, FreezeError> {
    fn sorted(value: serde_json::Value) -> serde_json::Value {
        use serde_json::Value;
        match value {
            Value::Object(map) => Value::Object(
                map.into_iter()
                    .collect::<BTreeMap<_, _>>()
                    .into_iter()
                    .map(|(k, v)| (k, sorted(v)))
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
            other => other,
        }
    }
    let value =
        serde_json::to_value(value).map_err(|e| FreezeError::new("fingerprint", e.to_string()))?;
    let bytes = serde_json::to_vec(&sorted(value))
        .map_err(|e| FreezeError::new("fingerprint", e.to_string()))?;
    let mut digest = Sha256::new();
    digest.update(b"kkscreeps/contribution-plan/v1\0");
    digest.update(bytes);
    Ok(PlanHash(digest.finalize().into()))
}
