//! 写入封套只保留候选成员；全部值仍按原 JSON 转换规则访问和校验。

use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// 拥有的写入候选；缺失 state 等同 null，缺失 target.fields 表示不写字段。
#[derive(Debug, Default)]
pub struct ChainWrite {
    pub state: Value,
    pub target_fields: Option<Value>,
}

#[derive(Clone, Copy)]
enum Envelope {
    Context,
    Target,
}

struct EnvelopeVisitor(Envelope);
struct TargetWrite(ChainWrite);

impl<'de> Deserialize<'de> for ChainWrite {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(EnvelopeVisitor(Envelope::Context))
    }
}

impl<'de> Deserialize<'de> for TargetWrite {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer
            .deserialize_any(EnvelopeVisitor(Envelope::Target))
            .map(Self)
    }
}

impl<'de> Visitor<'de> for EnvelopeVisitor {
    type Value = ChainWrite;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("any valid JSON value")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
        let mut write = ChainWrite::default();
        // 沿用 serde_v8 的动态 MapAccess，保持自有键/getter 的访问顺序。
        // 未使用成员也必须转成 Value；IgnoredAny 会漏掉转换错误与 getter 故障。
        while let Some(key) = map.next_key::<String>()? {
            if matches!(self.0, Envelope::Context) && key == "target" {
                write.target_fields = map.next_value::<TargetWrite>()?.0.target_fields;
            } else {
                let value = map.next_value::<Value>()?;
                match (self.0, key.as_str()) {
                    (Envelope::Context, "state") => write.state = value,
                    (Envelope::Target, "fields") => write.target_fields = Some(value),
                    _ => {}
                }
            }
        }
        Ok(write)
    }

    fn visit_seq<S: SeqAccess<'de>>(self, mut sequence: S) -> Result<Self::Value, S::Error> {
        // 非对象封套原本不含候选成员；元素仍完整转换，不保留无用外层数组。
        while sequence.next_element::<Value>()?.is_some() {}
        Ok(ChainWrite::default())
    }

    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
        Ok(ChainWrite::default())
    }
    fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
        Ok(ChainWrite::default())
    }
    fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
        Ok(ChainWrite::default())
    }
    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
        Ok(ChainWrite::default())
    }
    fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
        Ok(ChainWrite::default())
    }
    fn visit_string<E>(self, _: String) -> Result<Self::Value, E> {
        Ok(ChainWrite::default())
    }
    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(ChainWrite::default())
    }
    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(ChainWrite::default())
    }
    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deno_core::{JsRuntime, RuntimeOptions, v8};
    use serde::de::DeserializeOwned;

    fn convert<T: DeserializeOwned>(
        runtime: &mut JsRuntime,
        expression: &str,
    ) -> Result<T, String> {
        let input = runtime
            .execute_script(
                "write_fixture",
                format!("globalThis.trace=[];({expression})"),
            )
            .unwrap();
        deno_core::scope!(scope, runtime);
        v8::tc_scope!(let tc, scope);
        let input = v8::Local::new(tc, input);
        crate::v8_convert::from_v8(tc, input).map_err(|error| error.to_string())
    }

    fn trace(runtime: &mut JsRuntime) -> String {
        let value = runtime
            .execute_script("write_trace", "trace.join(',')")
            .unwrap();
        deno_core::scope!(scope, runtime);
        let value = v8::Local::new(scope, value);
        deno_core::serde_v8::from_v8(scope, value).unwrap()
    }

    #[test]
    fn v8_write_conversion_preserves_getter_order_shapes_and_unknown_value_faults() {
        let mut runtime = JsRuntime::new(RuntimeOptions::default());
        for expression in [
            "null",
            "undefined",
            "true",
            "123",
            "-3",
            "1.5",
            "NaN",
            "Infinity",
            "'text'",
            "[]",
            "[1,{x:2},null]",
            "new Uint8Array([1,2])",
            "new ArrayBuffer(2)",
            "1n",
            "({state:undefined,target:{fields:undefined}})",
            "Object.create({state:{inherited:true},target:{fields:{cooldown:3}}})",
            "({state:{},target:Object.create({fields:{cooldown:3}})})",
            "({state:{},target:[{fields:{cooldown:3}}]})",
            "new Map([['target',new Map([['fields',{cooldown:3}]] )],['state',{map:true}]])",
            "({get target(){trace.push('target');return {get fields(){trace.push('fields');return {cooldown:3}},get extra(){trace.push('target-extra');return []}}},get extra(){trace.push('root-extra');return 0},get state(){trace.push('state');return {value:1}}})",
            "new Proxy({target:{fields:{}},state:{}},{ownKeys(t){trace.push('keys');return Reflect.ownKeys(t)},get(t,k){trace.push(String(k));return Reflect.get(t,k)}})",
            "({state:{},target:{fields:{},get extra(){trace.push('target-extra');throw Error('target-getter')}}})",
            "({state:{},get extra(){trace.push('root-extra');throw Error('root-getter')}})",
            "({state:{},target:{fields:{},extra:1n}})",
            "({state:{},extra:1n})",
            "({state:{},extra:Symbol('bad')})",
            "(()=>{const loop={};loop.self=loop;return {state:{},target:loop}})()",
            "(()=>{const loop={};loop.self=loop;return {state:{},extra:loop}})()",
            "[1n]",
        ] {
            let original = convert::<Value>(&mut runtime, expression);
            let original_trace = trace(&mut runtime);
            let write = convert::<ChainWrite>(&mut runtime, expression);
            assert_eq!(trace(&mut runtime), original_trace, "{expression}");
            match (original, write) {
                (Ok(original), Ok(write)) => {
                    assert_eq!(
                        write.state,
                        original.get("state").cloned().unwrap_or_default(),
                        "{expression}"
                    );
                    assert_eq!(
                        write.target_fields,
                        original.pointer("/target/fields").cloned(),
                        "{expression}"
                    );
                }
                (Err(original), Err(write)) => assert_eq!(write, original, "{expression}"),
                (original, write) => panic!("{expression}: {original:?} != {write:?}"),
            }
        }
    }

    #[test]
    fn write_envelope_matches_original_json_extraction_including_duplicate_keys() {
        for text in [
            "null",
            "true",
            "0",
            "-1",
            "1.5",
            r#""text""#,
            "[]",
            "[{},null,3]",
            "{}",
            r#"{"state":null,"target":null}"#,
            r#"{"state":[1,2],"target":{"fields":null}}"#,
            r#"{"state":{"nested":true},"target":{"fields":{"cooldown":7},"id":"other"},"extra":[]}"#,
            r#"{"target":[{"fields":{}}]}"#,
            r#"{"target":{"fields":1,"fields":2},"state":1,"state":2}"#,
            r#"{"target":{"fields":{}},"target":null}"#,
        ] {
            let original: Value = serde_json::from_str(text).unwrap();
            let write: ChainWrite = serde_json::from_str(text).unwrap();
            assert_eq!(
                write.state,
                original.get("state").cloned().unwrap_or_default(),
                "{text}"
            );
            assert_eq!(
                write.target_fields,
                original.pointer("/target/fields").cloned(),
                "{text}"
            );
        }
    }
}
