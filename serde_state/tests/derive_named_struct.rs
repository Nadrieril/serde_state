use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_state::{DeserializeState, SerializeState};
use std::{cell::Cell, marker::PhantomData};

#[derive(Default)]
struct Recorder {
    serialized: Cell<usize>,
    deserialized: Cell<usize>,
}

trait RecorderLike {
    fn mark_serialized(&self);
    fn mark_deserialized(&self);
    fn serialized_count(&self) -> usize;
    fn deserialized_count(&self) -> usize;
}

impl RecorderLike for Recorder {
    fn mark_serialized(&self) {
        self.serialized.set(self.serialized.get() + 1);
    }

    fn mark_deserialized(&self) {
        self.deserialized.set(self.deserialized.get() + 1);
    }

    fn serialized_count(&self) -> usize {
        self.serialized.get()
    }

    fn deserialized_count(&self) -> usize {
        self.deserialized.get()
    }
}

#[derive(Clone, Debug, PartialEq)]
struct CounterValue(u32);

#[derive(Clone, Debug, PartialEq)]
struct GenericCounterValue(u32);

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Default)]
struct PlainValue(u32);

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

impl<State: RecorderLike + ?Sized> SerializeState<State> for GenericCounterValue {
    fn serialize_state<S>(&self, state: &State, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        state.mark_serialized();
        serializer.serialize_u32(self.0)
    }
}

impl<'de, State: RecorderLike + ?Sized> DeserializeState<'de, State> for GenericCounterValue {
    fn deserialize_state<D>(state: &State, deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        state.mark_deserialized();
        let value = u32::deserialize(deserializer)?;
        Ok(GenericCounterValue(value))
    }
}

mod counter_passthrough {
    use super::CounterValue;
    use serde::Deserialize;

    pub fn serialize_state<S, State: ?Sized>(
        value: &CounterValue,
        _state: &State,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u32(value.0 + 100)
    }

    pub fn deserialize_state<'de, State: ?Sized, D>(
        _state: &State,
        deserializer: D,
    ) -> Result<CounterValue, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let stored = u32::deserialize(deserializer)?;
        Ok(CounterValue(stored - 100))
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
    #[serde_state(stateless)]
    value: u32,
}

#[derive(SerializeState, DeserializeState, Debug, PartialEq)]
struct MixedModes {
    #[serde_state(stateless)]
    plain: PlainValue,
    stateful: CounterValue,
}

#[derive(SerializeState, DeserializeState, Debug, PartialEq)]
#[serde_state(stateless)]
struct StatelessContainerWithOverride {
    plain: PlainValue,
    #[serde_state(stateful)]
    counter: CounterValue,
}

#[derive(Clone, SerializeState, DeserializeState, Debug, PartialEq)]
#[serde_state(stateful)]
enum VariantModes {
    #[serde_state(stateless)]
    Plain(PlainValue),
    #[serde_state(stateless)]
    Struct {
        value: PlainValue,
    },
    Stateful(CounterValue),
    #[serde_state(stateless)]
    WithOverride {
        #[serde_state(stateful)]
        counter: CounterValue,
    },
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

#[derive(SerializeState, DeserializeState, Debug, PartialEq)]
struct RenamedAndSkipped {
    #[serde(rename = "external")]
    renamed: CounterValue,
    #[serde(skip)]
    skipped: PlainValue,
}

#[derive(SerializeState, DeserializeState, Debug, PartialEq)]
struct GenericContainer<T> {
    first: T,
    second: T,
}

#[derive(SerializeState, DeserializeState, Debug, PartialEq)]
#[serde_state(state_implements = RecorderLike)]
struct TraitBoundContainer {
    value: GenericCounterValue,
}

#[derive(SerializeState, DeserializeState, Debug, PartialEq)]
#[serde_state(state = Recorder)]
struct WithHelperField {
    #[serde(with = "counter_passthrough")]
    counter: CounterValue,
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
fn generic_struct_threads_state() {
    let value = GenericContainer {
        first: CounterValue(21),
        second: CounterValue(22),
    };
    let state = Recorder::default();
    let mut buffer = Vec::new();
    {
        let mut serializer = serde_json::Serializer::new(&mut buffer);
        value
            .serialize_state(&state, &mut serializer)
            .expect("generic serialization");
    }
    assert_eq!(state.serialized.get(), 2);

    let state = Recorder::default();
    let mut deserializer = serde_json::Deserializer::from_slice(&buffer);
    let decoded = GenericContainer::deserialize_state(&state, &mut deserializer).unwrap();
    assert_eq!(decoded, value);
    assert_eq!(state.deserialized.get(), 2);
}

#[test]
fn stateless_fields_use_plain_serde() {
    let value = MixedModes {
        plain: PlainValue(7),
        stateful: CounterValue(8),
    };

    let state = Recorder::default();
    let mut buffer = Vec::new();
    {
        let mut serializer = serde_json::Serializer::new(&mut buffer);
        value
            .serialize_state(&state, &mut serializer)
            .expect("mixed serialization");
    }
    assert_eq!(state.serialized.get(), 1);

    let state = Recorder::default();
    let mut deserializer = serde_json::Deserializer::from_slice(&buffer);
    let decoded = MixedModes::deserialize_state(&state, &mut deserializer).unwrap();
    assert_eq!(decoded, value);
    assert_eq!(state.deserialized.get(), 1);

    let value = StatelessContainerWithOverride {
        plain: PlainValue(9),
        counter: CounterValue(10),
    };

    let state = Recorder::default();
    let mut buffer = Vec::new();
    {
        let mut serializer = serde_json::Serializer::new(&mut buffer);
        value
            .serialize_state(&state, &mut serializer)
            .expect("stateless container serialization");
    }
    assert_eq!(state.serialized.get(), 1);

    let state = Recorder::default();
    let mut deserializer = serde_json::Deserializer::from_slice(&buffer);
    let decoded =
        StatelessContainerWithOverride::deserialize_state(&state, &mut deserializer).unwrap();
    assert_eq!(decoded, value);
    assert_eq!(state.deserialized.get(), 1);
}

#[test]
fn stateless_variants_control_state_usage() {
    fn round_trip(value: VariantModes) -> usize {
        let state = Recorder::default();
        let mut buffer = Vec::new();
        {
            let mut serializer = serde_json::Serializer::new(&mut buffer);
            value
                .serialize_state(&state, &mut serializer)
                .expect("variant serialization");
        }
        let mut deserializer = serde_json::Deserializer::from_slice(&buffer);
        let de_state = Recorder::default();
        let decoded = VariantModes::deserialize_state(&de_state, &mut deserializer).unwrap();
        assert_eq!(decoded, value);
        state.serialized.get()
    }

    assert_eq!(round_trip(VariantModes::Plain(PlainValue(1))), 0);
    assert_eq!(
        round_trip(VariantModes::Struct {
            value: PlainValue(2)
        }),
        0
    );
    assert_eq!(round_trip(VariantModes::Stateful(CounterValue(3))), 1);
    assert_eq!(
        round_trip(VariantModes::WithOverride {
            counter: CounterValue(4),
        }),
        1
    );
}

#[test]
fn serde_rename_and_skip_are_respected() {
    let value = RenamedAndSkipped {
        renamed: CounterValue(5),
        skipped: PlainValue(6),
    };

    let state = Recorder::default();
    let mut buffer = Vec::new();
    {
        let mut serializer = serde_json::Serializer::new(&mut buffer);
        value
            .serialize_state(&state, &mut serializer)
            .expect("rename serialization");
    }
    assert_eq!(state.serialized.get(), 1);
    let json_value: serde_json::Value = serde_json::from_slice(&buffer).unwrap();
    assert_eq!(json_value, json!({"external": 5}));

    let state = Recorder::default();
    let mut deserializer = serde_json::Deserializer::from_slice(&buffer);
    let decoded = RenamedAndSkipped::deserialize_state(&state, &mut deserializer).unwrap();
    assert_eq!(decoded.renamed, CounterValue(5));
    assert_eq!(decoded.skipped, PlainValue(0));
    assert_eq!(state.deserialized.get(), 1);
}

#[test]
fn state_implements_applies_trait_bounds() {
    let value = TraitBoundContainer {
        value: GenericCounterValue(12),
    };

    let state = Recorder::default();
    let mut buffer = Vec::new();
    {
        let mut serializer = serde_json::Serializer::new(&mut buffer);
        value
            .serialize_state(&state, &mut serializer)
            .expect("trait-bound serialization");
    }
    assert_eq!(state.serialized_count(), 1);
    let json_value: serde_json::Value = serde_json::from_slice(&buffer).unwrap();
    assert_eq!(json_value, json!({"value": 12}));

    let state = Recorder::default();
    let mut deserializer = serde_json::Deserializer::from_slice(&buffer);
    let decoded = TraitBoundContainer::deserialize_state(&state, &mut deserializer).unwrap();
    assert_eq!(decoded, value);
    assert_eq!(state.deserialized_count(), 1);
}

#[test]
fn serde_with_calls_custom_helpers() {
    let value = WithHelperField {
        counter: CounterValue(7),
    };

    let state = Recorder::default();
    let mut buffer = Vec::new();
    {
        let mut serializer = serde_json::Serializer::new(&mut buffer);
        value
            .serialize_state(&state, &mut serializer)
            .expect("with serialization");
    }
    assert_eq!(state.serialized.get(), 0);
    let json_value: serde_json::Value = serde_json::from_slice(&buffer).unwrap();
    assert_eq!(json_value, json!({"counter": 107}));

    let state = Recorder::default();
    let mut deserializer = serde_json::Deserializer::from_slice(&buffer);
    let decoded = WithHelperField::deserialize_state(&state, &mut deserializer).unwrap();
    assert_eq!(decoded.counter, CounterValue(7));
    assert_eq!(state.deserialized.get(), 0);
}

#[test]
fn recursive_enum_threads_state() {
    #[derive(SerializeState, DeserializeState, Debug, PartialEq)]
    #[serde_state(state = Recorder)]
    enum CounterList {
        Nil,
        Cons(CounterValue, Box<CounterList>),
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
    assert_eq!(state.serialized.get(), 2);
    let json_value: serde_json::Value = serde_json::from_slice(&buffer).unwrap();
    assert_eq!(json_value, json!({"Cons": [1, {"Cons": [2, "Nil"]}]}));

    let state = Recorder::default();
    let mut deserializer = serde_json::Deserializer::from_slice(&buffer);
    let decoded = CounterList::deserialize_state(&state, &mut deserializer).unwrap();
    assert_eq!(decoded, list);
    assert_eq!(state.deserialized.get(), 2);
}
