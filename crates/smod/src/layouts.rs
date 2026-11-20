//! Checked layout fingerprints recorded by the mod compiler.
use serde::Deserialize;
use std::{collections::BTreeMap, io::Read};

struct Unique<V>(BTreeMap<String, V>);
impl<'de, V: Deserialize<'de>> Deserialize<'de> for Unique<V> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor<V>(std::marker::PhantomData<V>);
        impl<'de, V: Deserialize<'de>> serde::de::Visitor<'de> for Visitor<V> {
            type Value = Unique<V>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an object with unique keys")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut a: A,
            ) -> Result<Self::Value, A::Error> {
                let mut map = BTreeMap::new();
                while let Some((key, value)) = a.next_entry::<String, V>()? {
                    if map.insert(key.clone(), value).is_some() {
                        return Err(serde::de::Error::custom(format!("duplicate key {key}")));
                    }
                }
                Ok(Unique(map))
            }
        }
        d.deserialize_map(Visitor(std::marker::PhantomData))
    }
}

pub const FILE: &str = "layouts.json";
const MAX_BYTES: u64 = 1 << 20;

/// Type fingerprints indexed by owning mod and fully qualified type name.
#[derive(Clone, Debug, Default)]
pub struct Layouts(BTreeMap<String, BTreeMap<String, String>>);

impl Layouts {
    /// Validate untrusted package metadata once, with a bounded read.
    pub fn read(reader: impl Read) -> Result<Self, String> {
        let mut bytes = Vec::new();
        reader
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("{FILE}: {e}"))?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(format!("{FILE}: exceeds 1 MiB"));
        }
        let parsed: Unique<Unique<String>> =
            serde_json::from_slice(&bytes).map_err(|e| format!("{FILE} is malformed: {e}"))?;
        let maps: BTreeMap<_, _> = parsed.0.into_iter().map(|(k, v)| (k, v.0)).collect();
        for (owner, types) in &maps {
            for (name, hash) in types {
                if hash.len() != 16
                    || !hash
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return Err(format!(
                        "{FILE}: {owner} type {name}: expected 16 lowercase hex digits"
                    ));
                }
            }
        }
        Ok(Self(maps))
    }

    /// First recorded library type whose layout changed, ordered by type name.
    pub fn first_difference<'a>(&'a self, library: &str, actual: &Self) -> Option<&'a str> {
        self.0.get(library)?.iter().find_map(|(name, hash)| {
            (actual.0.get(library).and_then(|types| types.get(name)) != Some(hash))
                .then_some(name.as_str())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_metadata_is_refused() {
        for bytes in [
            b"{".as_slice(),
            br#"{"lib":{},"lib":{}}"#,
            br#"{"lib":{"T":"0123456789abcdef","T":"0123456789abcdef"}}"#,
            br#"{"lib":{"T":"ABCDEF0123456789"}}"#,
            br#"{"lib":{"T":12}}"#,
            br#"{"lib":{"T":"0123"}}"#,
        ] {
            assert!(Layouts::read(bytes).unwrap_err().starts_with(FILE));
        }
        assert!(
            Layouts::read(vec![b' '; MAX_BYTES as usize + 1].as_slice())
                .unwrap_err()
                .contains("exceeds")
        );
    }
}
