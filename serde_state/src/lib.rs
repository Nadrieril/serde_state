use serde::Serialize;
pub use serde_state_derive::{DeserializeState, SerializeState};
use std::boxed::Box;
use std::marker::PhantomData;

pub trait SerializeState<State: ?Sized> {
    fn serialize_state<S>(&self, state: &State, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer;
}

pub trait DeserializeState<'de, State: ?Sized>: Sized {
    fn deserialize_state<D>(state: &State, deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>;
}

impl<State: ?Sized, T: SerializeState<State> + ?Sized> SerializeState<State> for Box<T> {
    fn serialize_state<S>(&self, state: &State, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        T::serialize_state(&**self, state, serializer)
    }
}

impl<'de, State: ?Sized, T> DeserializeState<'de, State> for Box<T>
where
    T: DeserializeState<'de, State>,
{
    fn deserialize_state<D>(state: &State, deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        T::deserialize_state(state, deserializer).map(Box::new)
    }
}

impl<State: ?Sized> SerializeState<State> for u32 {
    fn serialize_state<S>(&self, _state: &State, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u32(*self)
    }
}

impl<'de, State: ?Sized> DeserializeState<'de, State> for u32 {
    fn deserialize_state<D>(_state: &State, deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::Deserialize;
        u32::deserialize(deserializer)
    }
}

impl<State: ?Sized> SerializeState<State> for usize {
    fn serialize_state<S>(&self, _state: &State, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u64(*self as u64)
    }
}

impl<'de, State: ?Sized> DeserializeState<'de, State> for usize {
    fn deserialize_state<D>(_state: &State, deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::Deserialize;
        Ok(u32::deserialize(deserializer)? as usize)
    }
}

impl<State: ?Sized, T> SerializeState<State> for PhantomData<T> {
    fn serialize_state<S>(&self, _state: &State, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_unit_struct("PhantomData")
    }
}

impl<'de, State: ?Sized, T> DeserializeState<'de, State> for PhantomData<T> {
    fn deserialize_state<D>(_state: &State, deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::Deserialize;
        let _ = <serde::de::IgnoredAny>::deserialize(deserializer)?;
        Ok(PhantomData)
    }
}

/// A value with attached state. Its Serialize impl calls `T`'s SerializeState impl.
pub struct WithState<'state, T, State: ?Sized> {
    value: T,
    state: &'state State,
}

impl<'state, T, State> WithState<'state, T, State>
where
    State: ?Sized,
{
    pub fn new(value: T, state: &'state State) -> Self {
        Self { value, state }
    }
}

impl<T, State: ?Sized> Serialize for WithState<'_, T, State>
where
    T: SerializeState<State>,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.value.serialize_state(self.state, serializer)
    }
}

pub mod __private {
    use serde::de::DeserializeSeed;
    use serde::{Deserializer, Serialize, Serializer};

    use crate::{DeserializeState, SerializeState};

    pub struct SerializeRef<'state, T: ?Sized, State: ?Sized> {
        value: &'state T,
        state: &'state State,
    }

    impl<'state, T, State> SerializeRef<'state, T, State>
    where
        T: ?Sized,
        State: ?Sized,
    {
        pub fn new(value: &'state T, state: &'state State) -> Self {
            Self { value, state }
        }
    }

    impl<'state, T, State> Serialize for SerializeRef<'state, T, State>
    where
        T: SerializeState<State> + ?Sized,
        State: ?Sized,
    {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            self.value.serialize_state(self.state, serializer)
        }
    }

    pub fn wrap_serialize<'state, T, State>(
        value: &'state T,
        state: &'state State,
    ) -> SerializeRef<'state, T, State>
    where
        T: SerializeState<State> + ?Sized,
        State: ?Sized,
    {
        SerializeRef::new(value, state)
    }

    pub struct DeserializeStateSeed<'state, T, State: ?Sized> {
        state: &'state State,
        _marker: core::marker::PhantomData<T>,
    }

    impl<'state, T, State: ?Sized> DeserializeStateSeed<'state, T, State> {
        pub fn new(state: &'state State) -> Self {
            Self {
                state,
                _marker: core::marker::PhantomData,
            }
        }
    }

    impl<'de, 'state, T, State> DeserializeSeed<'de> for DeserializeStateSeed<'state, T, State>
    where
        T: DeserializeState<'de, State>,
        State: ?Sized,
    {
        type Value = T;

        fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            T::deserialize_state(self.state, deserializer)
        }
    }

    pub fn wrap_deserialize_seed<'state, T, State: ?Sized>(
        state: &'state State,
    ) -> DeserializeStateSeed<'state, T, State> {
        DeserializeStateSeed::new(state)
    }
}
