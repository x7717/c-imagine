//! imagine-registry — registry emulation for the mini-NeoForge runtime.
//!
//! Paper 26.3 is the kernel: its registries own the *runtime numeric ids* the
//! protocol uses. NeoForge 1.21.1 mods register content under their own
//! `ResourceLocation`s, which do not exist in Paper's registries. This crate
//! models both sides:
//!
//! * [`ResourceLocation`] — validated `namespace:path` (NeoForge-side key);
//! * [`RegistryId`] — the runtime id Paper assigns during injection;
//! * [`Registry`] — insertion-ordered registry with O(1) lookup by
//!   both `ResourceLocation` and `RegistryId`;
//! * [`RegistrySet`] — all vanilla registries the runtime tracks;
//! * [`MappingTable`] — the 1.21.1 → 26.3 translation layer for content
//!   that exists on both sides (renamed/bumped ids).
//!
//! Design constraint: **numeric ids are allocated once and never reused** —
//! the protocol path (F2V encoders) caches them, and Paper's client-bound
//! codec tables are append-only in practice. Remapping mid-session is a
//! correctness bug; `Registry` therefore has no remove operation.

use std::collections::HashMap;

/// Vanilla-style resource location: `namespace:path`, lowercase a-z0-9._-.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResourceLocation {
    namespace: String,
    path: String,
}

impl ResourceLocation {
    pub fn new(namespace: &str, path: &str) -> Result<Self, RegistryError> {
        if !valid_ns(namespace) {
            return Err(RegistryError::InvalidLocation {
                value: format!("{namespace}:{path}"),
                why: "namespace".into(),
            });
        }
        if !valid_path(path) {
            return Err(RegistryError::InvalidLocation {
                value: format!("{namespace}:{path}"),
                why: "path".into(),
            });
        }
        Ok(Self {
            namespace: namespace.into(),
            path: path.into(),
        })
    }

    pub fn parse(s: &str) -> Result<Self, RegistryError> {
        match s.split_once(':') {
            Some((ns, p)) => Self::new(ns, p),
            None => Self::new("minecraft", s),
        }
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn path(&self) -> &str {
        &self.path
    }
}

impl std::fmt::Display for ResourceLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.namespace, self.path)
    }
}

fn valid_ns(ns: &str) -> bool {
    !ns.is_empty()
        && ns.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.' || c == '-')
}

fn valid_path(p: &str) -> bool {
    !p.is_empty()
        && p.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-' | '/'))
}

/// Numeric runtime id (the value that goes on the wire in 26.3 packets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegistryId(pub u32);

/// Typed registry key for the set of tracked vanilla registries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VanillaRegistry {
    Block,
    Item,
    EntityType,
    BlockEntityType,
    ParticleType,
    MenuType,
    SoundEvent,
    Potion,
    Enchantment,
    MobEffect,
    Attribute,
    DataComponentType,
}

impl VanillaRegistry {
    /// Registry path used in vanilla/json registries (1.21.1 naming).
    pub fn path(self) -> &'static str {
        match self {
            VanillaRegistry::Block => "block",
            VanillaRegistry::Item => "item",
            VanillaRegistry::EntityType => "entity_type",
            VanillaRegistry::BlockEntityType => "block_entity_type",
            VanillaRegistry::ParticleType => "particle_type",
            VanillaRegistry::MenuType => "menu",
            VanillaRegistry::SoundEvent => "sound_event",
            VanillaRegistry::Potion => "potion",
            VanillaRegistry::Enchantment => "enchantment",
            VanillaRegistry::MobEffect => "mob_effect",
            VanillaRegistry::Attribute => "attribute",
            VanillaRegistry::DataComponentType => "data_component_type",
        }
    }
}

/// One registered object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registered {
    pub key: ResourceLocation,
    pub id: RegistryId,
    /// Which mod introduced this entry ("minecraft" for vanilla snapshots).
    pub owner: String,
}

/// Insertion-ordered registry with O(1) lookups in both directions.
#[derive(Debug, Default, Clone)]
pub struct Registry {
    name: &'static str,
    entries: Vec<Registered>,
    by_key: HashMap<ResourceLocation, usize>,
}

impl Registry {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            entries: Vec::new(),
            by_key: HashMap::new(),
        }
    }

    pub fn name(&self) -> &str {
        self.name
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Insert a new entry; returns the allocated runtime id.
    /// Re-inserting an existing key is a no-op returning the original id
    /// (matches `Registry#register` idempotence for reload scenarios).
    pub fn insert(
        &mut self,
        key: ResourceLocation,
        owner: impl Into<String>,
    ) -> RegistryId {
        if let Some(&idx) = self.by_key.get(&key) {
            return self.entries[idx].id;
        }
        let id = RegistryId(self.entries.len() as u32);
        let idx = self.entries.len();
        self.entries.push(Registered {
            key: key.clone(),
            id,
            owner: owner.into(),
        });
        self.by_key.insert(key, idx);
        id
    }

    pub fn get(&self, key: &ResourceLocation) -> Option<&Registered> {
        self.by_key.get(key).map(|&i| &self.entries[i])
    }

    pub fn by_id(&self, id: RegistryId) -> Option<&Registered> {
        self.entries.get(id.0 as usize).filter(|r| r.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Registered> {
        self.entries.iter()
    }

    /// Entries owned by one mod (for mod unload/debug dumps).
    pub fn owned_by<'a>(&'a self, owner: &'a str) -> impl Iterator<Item = &'a Registered> + 'a {
        self.entries.iter().filter(move |r| r.owner == owner)
    }
}

/// Every tracked registry, keyed by [`VanillaRegistry`].
#[derive(Debug, Clone)]
pub struct RegistrySet {
    registries: HashMap<&'static str, Registry>,
}

impl Default for RegistrySet {
    fn default() -> Self {
        Self::new()
    }
}

impl RegistrySet {
    pub fn new() -> Self {
        let mut set = RegistrySet {
            registries: HashMap::new(),
        };
        let all = [
            VanillaRegistry::Block,
            VanillaRegistry::Item,
            VanillaRegistry::EntityType,
            VanillaRegistry::BlockEntityType,
            VanillaRegistry::ParticleType,
            VanillaRegistry::MenuType,
            VanillaRegistry::SoundEvent,
            VanillaRegistry::Potion,
            VanillaRegistry::Enchantment,
            VanillaRegistry::MobEffect,
            VanillaRegistry::Attribute,
            VanillaRegistry::DataComponentType,
        ];
        for reg in all {
            set.registries
                .insert(reg.path(), Registry::new(reg.path()));
        }
        set
    }

    pub fn registry(&self, which: VanillaRegistry) -> &Registry {
        self.registries
            .get(which.path())
            .expect("tracked registry always present")
    }

    pub fn registry_mut(&mut self, which: VanillaRegistry) -> &mut Registry {
        self.registries
            .entry(which.path())
            .or_insert_with(|| Registry::new(which.path()))
    }
}

/// 1.21.1 → 26.3 id mapping table for vanilla content that survived between
/// the two versions (renames live here; removals simply have no row).
#[derive(Debug, Default, Clone)]
pub struct MappingTable {
    /// (registry, old_key) -> new_key in the 26.3 runtime.
    renamed: HashMap<(VanillaRegistry, ResourceLocation), ResourceLocation>,
    /// Explicit numeric overrides where 26.3 protocol ids are known-ahead.
    protocol_ids: HashMap<(VanillaRegistry, ResourceLocation), RegistryId>,
}

impl MappingTable {
    pub fn rename(
        &mut self,
        reg: VanillaRegistry,
        from: &str,
        to: &str,
    ) -> Result<(), RegistryError> {
        let from = ResourceLocation::parse(from)?;
        let to = ResourceLocation::parse(to)?;
        self.renamed.insert((reg, from), to);
        Ok(())
    }

    pub fn set_protocol_id(
        &mut self,
        reg: VanillaRegistry,
        key: &str,
        id: u32,
    ) -> Result<(), RegistryError> {
        let key = ResourceLocation::parse(key)?;
        self.protocol_ids.insert((reg, key), RegistryId(id));
        Ok(())
    }

    pub fn translate(&self, reg: VanillaRegistry, key: &ResourceLocation) -> Option<&ResourceLocation> {
        self.renamed.get(&(reg, key.clone()))
    }

    pub fn protocol_id(&self, reg: VanillaRegistry, key: &ResourceLocation) -> Option<RegistryId> {
        self.protocol_ids.get(&(reg, key.clone())).copied()
    }

    pub fn len(&self) -> usize {
        self.renamed.len() + self.protocol_ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error("invalid resource location '{value}' ({why})")]
    InvalidLocation { value: String, why: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_location_validation() {
        assert!(ResourceLocation::new("examplemod", "ruby_ore").is_ok());
        assert!(ResourceLocation::new("examplemod", "items/ruby").is_ok());
        assert!(ResourceLocation::parse("no-ns-entry").unwrap().namespace() == "minecraft");
        assert!(ResourceLocation::new("BadNS", "x").is_err());
        assert!(ResourceLocation::new("ns", "UPPER").is_err());
        assert!(ResourceLocation::new("", "x").is_err());
    }

    #[test]
    fn registry_allocates_stable_ids() {
        let mut reg = Registry::new("block");
        let key = ResourceLocation::new("examplemod", "ruby_block").unwrap();
        let id1 = reg.insert(key.clone(), "examplemod");
        let id2 = reg.insert(key.clone(), "examplemod");
        assert_eq!(id1, id2, "idempotent insert");
        assert_eq!(reg.get(&key).unwrap().owner, "examplemod");
        assert_eq!(reg.by_id(id1).unwrap().key, key);
        assert!(reg.owned_by("examplemod").count() == 1);
        assert!(reg.owned_by("othermod").count() == 0);
    }

    #[test]
    fn registry_set_lookup() {
        let mut set = RegistrySet::default();
        let key = ResourceLocation::new("examplemod", "ruby").unwrap();
        set.registry_mut(VanillaRegistry::Item).insert(key.clone(), "examplemod");
        assert!(set.registry(VanillaRegistry::Item).get(&key).is_some());
        assert!(set.registry(VanillaRegistry::Block).is_empty());
    }

    #[test]
    fn mapping_table_translate() {
        let mut mt = MappingTable::default();
        mt.rename(VanillaRegistry::Block, "minecraft:old_block", "minecraft:new_block").unwrap();
        let old = ResourceLocation::parse("minecraft:old_block").unwrap();
        assert_eq!(
            mt.translate(VanillaRegistry::Block, &old).unwrap(),
            &ResourceLocation::parse("minecraft:new_block").unwrap()
        );
        assert!(mt.translate(VanillaRegistry::Block, &ResourceLocation::parse("minecraft:untouched").unwrap()).is_none());
    }

    #[test]
    fn protocol_id_overrides() {
        let mut mt = MappingTable::default();
        mt.set_protocol_id(VanillaRegistry::Item, "minecraft:diamond", 42).unwrap();
        assert_eq!(
            mt.protocol_id(VanillaRegistry::Item, &ResourceLocation::parse("minecraft:diamond").unwrap()),
            Some(RegistryId(42))
        );
    }
}
