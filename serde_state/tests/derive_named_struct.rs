use serde::Deserialize;
use serde_json::json;
use serde_state::{DeserializeState, SerializeState};
use std::cell::Cell;

#[derive(Default)]
struct Recorder {
    serialized: Cell<usize>,
    deserialized: Cell<usize>,
}

#[derive(Clone, Debug, PartialEq)]
struct CounterValue(u32);

impl SerializeState<Recorder> for CounterValue {
    fn serialize_state<S>(&self, state: &Recorder, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        state.serialized.set(state.serialized.get() + 1);
        serializer.serialize_u32(self.0)
    }
}

impl<'de> DeserializeState<'de, Recorder> for CounterValue {
    fn deserialize_state<D>(state: &Recorder, deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        state.deserialized.set(state.deserialized.get() + 1);
        let value = u32::deserialize(deserializer)?;
        Ok(CounterValue(value))
    }
}

#[derive(SerializeState, DeserializeState, Debug, PartialEq)]
#[serde_state(state = Recorder)]
struct Example {
    first: CounterValue,
    second: CounterValue,
}

#[derive(SerializeState, DeserializeState, Debug, PartialEq)]
#[serde(transparent)]
#[serde_state(state = Recorder)]
struct Wrapper {
    inner: CounterValue,
}

#[test]
fn serialize_named_struct_threads_state() {
    let value = Example {
        first: CounterValue(1),
        second: CounterValue(2),
    };
    let state = Recorder::default();
    let mut buffer = Vec::new();
    {
        let mut serializer = serde_json::Serializer::new(&mut buffer);
        value
            .serialize_state(&state, &mut serializer)
            .expect("serialization should succeed");
    }
    assert_eq!(state.serialized.get(), 2);
    let json_value: serde_json::Value = serde_json::from_slice(&buffer).unwrap();
    assert_eq!(json_value, json!({ "first": 1, "second": 2 }));
}

#[test]
fn deserialize_named_struct_threads_state() {
    let state = Recorder::default();
    let json = r#"{"first":3,"second":4}"#;
    let mut deserializer = serde_json::Deserializer::from_str(json);
    let decoded = Example::deserialize_state(&state, &mut deserializer).unwrap();
    assert_eq!(
        decoded,
        Example {
            first: CounterValue(3),
            second: CounterValue(4),
        }
    );
    assert_eq!(state.deserialized.get(), 2);
}

#[test]
fn transparent_struct_behaves_like_inner_value() {
    let state = Recorder::default();
    let wrapper = Wrapper {
        inner: CounterValue(11),
    };
    let mut buffer = Vec::new();
    {
        let mut serializer = serde_json::Serializer::new(&mut buffer);
        wrapper
            .serialize_state(&state, &mut serializer)
            .expect("transparent serialization");
    }
    assert_eq!(state.serialized.get(), 1);
    let json_value: serde_json::Value = serde_json::from_slice(&buffer).unwrap();
    assert_eq!(json_value, json!(11));

    let state = Recorder::default();
    let mut deserializer = serde_json::Deserializer::from_slice(&buffer);
    let decoded = Wrapper::deserialize_state(&state, &mut deserializer).unwrap();
    assert_eq!(
        decoded,
        Wrapper {
            inner: CounterValue(11)
        }
    );
    assert_eq!(state.deserialized.get(), 1);
}
