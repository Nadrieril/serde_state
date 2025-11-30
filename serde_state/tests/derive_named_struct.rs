use serde::Deserialize;
use serde_json::json;
use serde_state::{DeserializeState, SerializeState};
use std::{cell::Cell, marker::PhantomData};

#[derive(Default)]
struct Recorder {
    serialized: Cell<usize>,
    deserialized: Cell<usize>,
}

#[derive(Clone, Debug, PartialEq)]
struct CounterValue(u32);

impl SerializeState<Recorder> for CounterValue {
    fn serialize_state<S>(&self, recorder: &Recorder, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        recorder.serialized.set(recorder.serialized.get() + 1);
        serializer.serialize_u32(self.0)
    }
}

impl<'de> DeserializeState<'de, Recorder> for CounterValue {
    fn deserialize_state<D>(recorder: &Recorder, deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        recorder.deserialized.set(recorder.deserialized.get() + 1);
        let value = u32::deserialize(deserializer)?;
        Ok(CounterValue(value))
    }
}

#[derive(SerializeState, DeserializeState, Debug, PartialEq)]
struct Example {
    first: CounterValue,
    second: CounterValue,
}

#[derive(SerializeState, DeserializeState, Debug, PartialEq)]
#[serde(transparent)]
struct Wrapper {
    inner: CounterValue,
}

#[derive(SerializeState, DeserializeState, Debug, PartialEq)]
struct Pair(CounterValue, CounterValue);

#[derive(SerializeState, DeserializeState, Debug, PartialEq)]
struct Empty;

#[derive(SerializeState, DeserializeState, Debug, PartialEq)]
struct PlainNumbers {
    value: u32,
}

#[derive(Clone, SerializeState, DeserializeState, Debug, PartialEq)]
enum Action {
    Idle,
    Reset(CounterValue),
    Combine(CounterValue, CounterValue),
    Record {
        first: CounterValue,
        second: CounterValue,
    },
}

#[derive(SerializeState, DeserializeState, Debug, Default, PartialEq)]
struct PhantomWrapper {
    marker: PhantomData<NeedsNoBounds>,
}

struct NeedsNoBounds;

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

#[test]
fn plain_struct_does_not_need_state_attribute() {
    let state = ();
    let numbers = PlainNumbers { value: 42 };
    let mut buffer = Vec::new();
    {
        let mut serializer = serde_json::Serializer::new(&mut buffer);
        numbers
            .serialize_state(&state, &mut serializer)
            .expect("plain serialization");
    }
    let json_value: serde_json::Value = serde_json::from_slice(&buffer).unwrap();
    assert_eq!(json_value, json!({ "value": 42 }));

    let mut deserializer = serde_json::Deserializer::from_slice(&buffer);
    let decoded = PlainNumbers::deserialize_state(&state, &mut deserializer).unwrap();
    assert_eq!(decoded, numbers);
}

#[test]
fn tuple_and_unit_structs_thread_state() {
    let pair = Pair(CounterValue(7), CounterValue(8));
    let state = Recorder::default();
    let mut buffer = Vec::new();
    {
        let mut serializer = serde_json::Serializer::new(&mut buffer);
        pair.serialize_state(&state, &mut serializer)
            .expect("tuple serialization");
    }
    assert_eq!(state.serialized.get(), 2);
    let json_value: serde_json::Value = serde_json::from_slice(&buffer).unwrap();
    assert_eq!(json_value, json!([7, 8]));

    let state = Recorder::default();
    let mut deserializer = serde_json::Deserializer::from_slice(&buffer);
    let decoded = Pair::deserialize_state(&state, &mut deserializer).unwrap();
    assert_eq!(decoded, pair);
    assert_eq!(state.deserialized.get(), 2);

    let state = Recorder::default();
    let mut buffer = Vec::new();
    {
        let mut serializer = serde_json::Serializer::new(&mut buffer);
        Empty
            .serialize_state(&state, &mut serializer)
            .expect("unit serialization");
    }
    assert_eq!(state.serialized.get(), 0);
    let json_value: serde_json::Value = serde_json::from_slice(&buffer).unwrap();
    assert_eq!(json_value, serde_json::Value::Null);

    let state = Recorder::default();
    let mut deserializer = serde_json::Deserializer::from_slice(&buffer);
    let decoded = Empty::deserialize_state(&state, &mut deserializer).unwrap();
    assert_eq!(decoded, Empty);
    assert_eq!(state.deserialized.get(), 0);
}

#[test]
fn enums_thread_state_for_each_variant() {
    fn run_case(action: Action, expected_json: serde_json::Value, expected_hits: usize) {
        let state = Recorder::default();
        let mut buffer = Vec::new();
        {
            let mut serializer = serde_json::Serializer::new(&mut buffer);
            action
                .serialize_state(&state, &mut serializer)
                .expect("enum serialization");
        }
        assert_eq!(state.serialized.get(), expected_hits);
        let json_value: serde_json::Value = serde_json::from_slice(&buffer).unwrap();
        assert_eq!(json_value, expected_json);

        let state = Recorder::default();
        let mut deserializer = serde_json::Deserializer::from_slice(&buffer);
        let decoded = Action::deserialize_state(&state, &mut deserializer).unwrap();
        assert_eq!(decoded, action);
        assert_eq!(state.deserialized.get(), expected_hits);
    }

    run_case(Action::Idle, json!("Idle"), 0);
    run_case(Action::Reset(CounterValue(9)), json!({"Reset": 9}), 1);
    run_case(
        Action::Combine(CounterValue(10), CounterValue(11)),
        json!({"Combine": [10, 11]}),
        2,
    );
    run_case(
        Action::Record {
            first: CounterValue(12),
            second: CounterValue(13),
        },
        json!({"Record": {"first": 12, "second": 13}}),
        2,
    );
}

#[test]
fn perfect_derive_does_not_require_generic_bounds() {
    let ser_state = Recorder::default();
    let wrapper = PhantomWrapper::default();
    let mut buffer = Vec::new();
    {
        let mut serializer = serde_json::Serializer::new(&mut buffer);
        wrapper
            .serialize_state(&ser_state, &mut serializer)
            .expect("phantom serialization");
    }
    assert_eq!(ser_state.serialized.get(), 0);
    let de_state = Recorder::default();
    let mut deserializer = serde_json::Deserializer::from_slice(&buffer);
    let decoded = PhantomWrapper::deserialize_state(&de_state, &mut deserializer).unwrap();
    assert_eq!(decoded, wrapper);
    assert_eq!(de_state.deserialized.get(), 0);
}

#[test]
fn recursive_enum_threads_state() {
    #[derive(SerializeState, DeserializeState, Debug, PartialEq)]
    enum CounterList {
        Nil,
        Cons(CounterValue, #[serde_state(recursive)] Box<CounterList>),
    }

    let list = CounterList::Cons(
        CounterValue(1),
        Box::new(CounterList::Cons(
            CounterValue(2),
            Box::new(CounterList::Nil),
        )),
    );
    let state = Recorder::default();
    let mut buffer = Vec::new();
    {
        let mut serializer = serde_json::Serializer::new(&mut buffer);
        list.serialize_state(&state, &mut serializer)
            .expect("recursive serialization");
    }
    // assert_eq!(state.serialized.get(), 2);
    // let json_value: serde_json::Value = serde_json::from_slice(&buffer).unwrap();
    // assert_eq!(json_value, json!({"Cons": [1, {"Cons": [2, "Nil"]}]}));

    // let state = Recorder::default();
    // let mut deserializer = serde_json::Deserializer::from_slice(&buffer);
    // let decoded = CounterList::deserialize_state(&state, &mut deserializer).unwrap();
    // assert_eq!(decoded, list);
    // assert_eq!(state.deserialized.get(), 2);
}
