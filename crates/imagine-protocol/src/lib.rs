//! imagine-protocol — wire-level primitives + translation scaffolding.
//!
//! Two protocol worlds meet here:
//!
//! * **26.3** — the Paper kernel's client-bound protocol (vanilla clients);
//! * **1.21.1** — the NeoForge-era content the mods were built for.
//!
//! The F2V bridge (see `docs/F2V-RS.md`) rewrites modded content into
//! vanilla-visible streams: custom entities become 26.3 entity spawns with
//! remapped types + metadata, custom blocks become vanilla blocks with block
//! entity data, custom items become vanilla items with patched components.
//! Everything starts from the primitives in this crate: VarInt/VarLong,
//! length-prefixed strings, and a direction-tagged translation pipeline.

pub mod varint;

use imagine_registry::{RegistryId, ResourceLocation, VanillaRegistry};

/// Packet flow direction as seen by the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    ClientBound,
    ServerBound,
}

/// What to do with a packet intercepted by the CRUSSTY network brick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Drop,
    Disconnect,
}

/// A single content translation decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentBinding {
    /// How the mod refers to the content (1.21.1 world).
    pub modded: ResourceLocation,
    /// What vanilla 26.3 clients actually see.
    pub visible_as: ResourceLocation,
    pub registry: VanillaRegistry,
}

/// Shared translation state for one server session.
#[derive(Debug, Default, Clone)]
pub struct Translator {
    bindings: Vec<ContentBinding>,
    /// modded key -> index into bindings (O(1) lookup on hot packet path).
    index: std::collections::HashMap<(u8, String), usize>,
}

impl Translator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a binding: modded content -> vanilla-visible content.
    pub fn bind(&mut self, b: ContentBinding) {
        let idx = self.bindings.len();
        let key = (discriminant(b.registry), b.modded.to_string());
        self.index.insert(key, idx);
        self.bindings.push(b);
    }

    /// Resolve what a modded key is visible as on the wire.
    pub fn resolve(
        &self,
        registry: VanillaRegistry,
        modded: &ResourceLocation,
    ) -> Option<&ContentBinding> {
        let key = (discriminant(registry), modded.to_string());
        self.index.get(&key).map(|&i| &self.bindings[i])
    }

    pub fn bindings(&self) -> &[ContentBinding] {
        &self.bindings
    }

    /// Convenience: protocol id for the visible content once the Paper-side
    /// registry has assigned one.
    pub fn visible_id(
        &self,
        mapping: &imagine_registry::MappingTable,
        registry: VanillaRegistry,
        modded: &ResourceLocation,
    ) -> Option<RegistryId> {
        let b = self.resolve(registry, modded)?;
        mapping.protocol_id(registry, &b.visible_as)
    }
}

fn discriminant(r: VanillaRegistry) -> u8 {
    match r {
        VanillaRegistry::Block => 0,
        VanillaRegistry::Item => 1,
        VanillaRegistry::EntityType => 2,
        VanillaRegistry::BlockEntityType => 3,
        VanillaRegistry::ParticleType => 4,
        VanillaRegistry::MenuType => 5,
        VanillaRegistry::SoundEvent => 6,
        VanillaRegistry::Potion => 7,
        VanillaRegistry::Enchantment => 8,
        VanillaRegistry::MobEffect => 9,
        VanillaRegistry::Attribute => 10,
        VanillaRegistry::DataComponentType => 11,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use imagine_registry::MappingTable;

    fn rl(s: &str) -> ResourceLocation {
        ResourceLocation::parse(s).unwrap()
    }

    #[test]
    fn bind_and_resolve() {
        let mut t = Translator::new();
        t.bind(ContentBinding {
            modded: rl("examplemod:ruby_golem"),
            visible_as: rl("minecraft:iron_golem"),
            registry: VanillaRegistry::EntityType,
        });
        let r = t
            .resolve(VanillaRegistry::EntityType, &rl("examplemod:ruby_golem"))
            .expect("binding");
        assert_eq!(r.visible_as, rl("minecraft:iron_golem"));
        assert!(t.resolve(VanillaRegistry::Item, &rl("examplemod:ruby_golem")).is_none());
    }

    #[test]
    fn visible_id_through_mapping() {
        let mut t = Translator::new();
        t.bind(ContentBinding {
            modded: rl("examplemod:ruby_ore"),
            visible_as: rl("minecraft:redstone_ore"),
            registry: VanillaRegistry::Block,
        });
        let mut mt = MappingTable::default();
        mt.set_protocol_id(VanillaRegistry::Block, "minecraft:redstone_ore", 77).unwrap();
        assert_eq!(
            t.visible_id(&mt, VanillaRegistry::Block, &rl("examplemod:ruby_ore")),
            Some(RegistryId(77))
        );
    }
}
