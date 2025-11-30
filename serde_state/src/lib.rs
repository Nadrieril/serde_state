pub use serde_state_derive::{DeserializeState, SerializeState};

pub trait SerializeState<State: ?Sized> {
    fn serialize_state<S>(&self, state: &mut State, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer;
}

pub trait DeserializeState<'de, State: ?Sized>: Sized {
    fn deserialize_state<D>(state: &mut State, deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>;
}

// Blanket impls for normal serde types.
impl<T: serde::Serialize, State: ?Sized> SerializeState<State> for T {
    fn serialize_state<S>(&self, _state: &mut State, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.serialize(serializer)
    }
}
impl<'de, T: serde::Deserialize<'de>, State: ?Sized> DeserializeState<'de, State> for T {
    fn deserialize_state<D>(_state: &mut State, deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        T::deserialize(deserializer)
    }
}
