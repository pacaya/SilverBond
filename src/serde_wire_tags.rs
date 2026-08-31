use std::{
    error::Error as StdError,
    fmt::{self, Display},
};

use serde::de::{self, Deserialize, Deserializer, Visitor};

use crate::model::{WorkflowEdgeOutcome, WorkflowNodeType};

#[derive(Debug)]
struct HarvestError;

impl StdError for HarvestError {}

impl Display for HarvestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "harvest")
    }
}

impl de::Error for HarvestError {
    fn custom<T: Display>(_msg: T) -> Self {
        HarvestError
    }
}

struct EnumVariantProbe<'a> {
    tags: &'a mut Option<&'static [&'static str]>,
}

impl<'de> Deserializer<'de> for EnumVariantProbe<'de> {
    type Error = HarvestError;

    fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(HarvestError)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        variants: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        *self.tags = Some(variants);
        Err(HarvestError)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct identifier ignored_any
    }
}

fn harvest_serde_enum_tags<T>() -> &'static [&'static str]
where
    T: for<'de> Deserialize<'de>,
{
    let mut tags = None;
    let probe = EnumVariantProbe { tags: &mut tags };
    let _ = T::deserialize(probe);
    tags.expect("serde enum variant harvest")
}

pub fn workflow_node_type_tags() -> &'static [&'static str] {
    harvest_serde_enum_tags::<WorkflowNodeType>()
}

pub fn workflow_edge_outcome_tags() -> &'static [&'static str] {
    harvest_serde_enum_tags::<WorkflowEdgeOutcome>()
}
