use std::{
    collections::{BTreeMap, HashMap},
    ops::Deref,
    sync::Arc,
};

use serde::Deserialize;

#[derive(Debug, Clone)]
pub enum RawString<'a> {
    Borrowed(&'a str),
    Owned(Arc<str>),
}

impl<'a> RawString<'a> {
    pub const fn borrowed(value: &'a str) -> Self {
        Self::Borrowed(value)
    }
}

impl Deref for RawString<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(value) => value,
            Self::Owned(value) => value,
        }
    }
}

impl AsRef<str> for RawString<'_> {
    fn as_ref(&self) -> &str {
        self
    }
}

impl From<String> for RawString<'static> {
    fn from(value: String) -> Self {
        Self::Owned(Arc::from(value))
    }
}

impl<'a> From<&'a str> for RawString<'a> {
    fn from(value: &'a str) -> Self {
        Self::Borrowed(value)
    }
}

impl<'de, 'a> Deserialize<'de> for RawString<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer)
            .map(|value| Self::Owned(Arc::from(value)))
    }
}

#[derive(Debug, Clone)]
pub enum RawList<'a, T> {
    Borrowed(&'a [T]),
    Owned(Vec<T>),
}

impl<T> Default for RawList<'_, T> {
    fn default() -> Self {
        Self::Owned(Vec::new())
    }
}

impl<T> From<Vec<T>> for RawList<'static, T> {
    fn from(value: Vec<T>) -> Self {
        Self::Owned(value)
    }
}

impl<'a, T> RawList<'a, T> {
    pub const EMPTY: Self = Self::Borrowed(&[]);

    pub const fn borrowed(values: &'a [T]) -> Self {
        Self::Borrowed(values)
    }
}

impl<T> Deref for RawList<'_, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(values) => values,
            Self::Owned(values) => values,
        }
    }
}

impl<'de, 'a, T> Deserialize<'de> for RawList<'a, T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Vec::deserialize(deserializer).map(Self::Owned)
    }
}

#[derive(Debug, Clone)]
pub struct RawMapEntry<'a, T> {
    pub key: &'a str,
    pub value: T,
}

impl<T> Default for RawMap<'_, T> {
    fn default() -> Self {
        Self::Owned(BTreeMap::new())
    }
}

impl<T> From<HashMap<String, T>> for RawMap<'static, T> {
    fn from(value: HashMap<String, T>) -> Self {
        Self::Owned(value.into_iter().collect())
    }
}

impl<T> From<BTreeMap<String, T>> for RawMap<'static, T> {
    fn from(value: BTreeMap<String, T>) -> Self {
        Self::Owned(value)
    }
}

impl<'a, T> RawMapEntry<'a, T> {
    pub const fn new(key: &'a str, value: T) -> Self {
        Self { key, value }
    }
}

#[derive(Debug, Clone)]
pub enum RawMap<'a, T> {
    Borrowed(&'a [RawMapEntry<'a, T>]),
    Owned(BTreeMap<String, T>),
}

impl<'a, T> RawMap<'a, T> {
    pub const EMPTY: Self = Self::Borrowed(&[]);

    pub const fn borrowed(values: &'a [RawMapEntry<'a, T>]) -> Self {
        Self::Borrowed(values)
    }

    pub fn get(&self, key: &str) -> Option<&T> {
        match self {
            Self::Borrowed(values) => values
                .iter()
                .find(|entry| entry.key == key)
                .map(|entry| &entry.value),
            Self::Owned(values) => values.get(key),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::Borrowed(values) => values.is_empty(),
            Self::Owned(values) => values.is_empty(),
        }
    }

    pub fn iter(&self) -> RawMapIter<'_, 'a, T> {
        match self {
            Self::Borrowed(values) => RawMapIter::Borrowed(values.iter()),
            Self::Owned(values) => RawMapIter::Owned(values.iter()),
        }
    }
}

impl<'de, 'a, T> Deserialize<'de> for RawMap<'a, T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        BTreeMap::deserialize(deserializer).map(Self::Owned)
    }
}

pub enum RawMapIter<'b, 'a, T> {
    Borrowed(std::slice::Iter<'b, RawMapEntry<'a, T>>),
    Owned(std::collections::btree_map::Iter<'b, String, T>),
}

impl<'b, T> Iterator for RawMapIter<'b, '_, T> {
    type Item = (&'b str, &'b T);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Borrowed(values) => {
                values.next().map(|entry| (entry.key, &entry.value))
            }
            Self::Owned(values) => {
                values.next().map(|(key, value)| (key.as_str(), value))
            }
        }
    }
}
