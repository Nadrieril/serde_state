use crate::{DeserializeState, SerializeState};
use std::ops::{
    Range as StdRange, RangeFrom as StdRangeFrom, RangeInclusive as StdRangeInclusive,
    RangeTo as StdRangeTo,
};

#[derive(crate::SerializeState, crate::DeserializeState)]
struct Range<Idx> {
    start: Idx,
    end: Idx,
}

#[derive(crate::SerializeState, crate::DeserializeState)]
struct RangeFrom<Idx> {
    start: Idx,
}

#[derive(crate::SerializeState, crate::DeserializeState)]
struct RangeInclusive<Idx> {
    start: Idx,
    end: Idx,
}

#[derive(crate::SerializeState, crate::DeserializeState)]
struct RangeTo<Idx> {
    end: Idx,
}

impl<State: ?Sized, Idx> SerializeState<State> for StdRange<Idx>
where
    Idx: SerializeState<State>,
{
    fn serialize_state<S>(&self, state: &State, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        Range {
            start: &self.start,
            end: &self.end,
        }
        .serialize_state(state, serializer)
    }
}

impl<'de, State: ?Sized, Idx> DeserializeState<'de, State> for StdRange<Idx>
where
    Idx: DeserializeState<'de, State>,
{
    fn deserialize_state<D>(state: &State, deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let Range { start, end } = Range::deserialize_state(state, deserializer)?;
        Ok(start..end)
    }
}

impl<State: ?Sized, Idx> SerializeState<State> for StdRangeFrom<Idx>
where
    Idx: SerializeState<State>,
{
    fn serialize_state<S>(&self, state: &State, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        RangeFrom { start: &self.start }.serialize_state(state, serializer)
    }
}

impl<'de, State: ?Sized, Idx> DeserializeState<'de, State> for StdRangeFrom<Idx>
where
    Idx: DeserializeState<'de, State>,
{
    fn deserialize_state<D>(state: &State, deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let RangeFrom { start } = RangeFrom::deserialize_state(state, deserializer)?;
        Ok(start..)
    }
}

impl<State: ?Sized, Idx> SerializeState<State> for StdRangeInclusive<Idx>
where
    Idx: SerializeState<State>,
{
    fn serialize_state<S>(&self, state: &State, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        RangeInclusive {
            start: self.start(),
            end: self.end(),
        }
        .serialize_state(state, serializer)
    }
}

impl<'de, State: ?Sized, Idx> DeserializeState<'de, State> for StdRangeInclusive<Idx>
where
    Idx: DeserializeState<'de, State>,
{
    fn deserialize_state<D>(state: &State, deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let RangeInclusive { start, end } = RangeInclusive::deserialize_state(state, deserializer)?;
        Ok(StdRangeInclusive::new(start, end))
    }
}

impl<State: ?Sized, Idx> SerializeState<State> for StdRangeTo<Idx>
where
    Idx: SerializeState<State>,
{
    fn serialize_state<S>(&self, state: &State, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        RangeTo { end: &self.end }.serialize_state(state, serializer)
    }
}

impl<'de, State: ?Sized, Idx> DeserializeState<'de, State> for StdRangeTo<Idx>
where
    Idx: DeserializeState<'de, State>,
{
    fn deserialize_state<D>(state: &State, deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let RangeTo { end } = RangeTo::deserialize_state(state, deserializer)?;
        Ok(..end)
    }
}
