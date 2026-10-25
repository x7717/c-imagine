# F2V-RS — the visual bridge (modded content on vanilla clients)

F2V ("Forge-to-Vanilla", the original proof-of-concept) allowed a *vanilla*
client to connect to a modded server and see modded mobs and blocks. The
first Java implementation worked but was visually rough — this crate family
reimplements the idea natively on the CRUSSTY network brick with clean
protocol translation.

## Goal

A vanilla 26.3 client connecting to Paper+imagine must see, for every
NeoForge 1.21.1 mod:

- custom entities (spawn, metadata, position, animations where vanilla
  types support them),
- custom blocks (appear in-world, correct drop/item behavior),
- custom items (in inventory, in hand, on the ground),
- GUIs/containers (M1-adjacent: menus translate to vanilla menu types).

## Translation strategy

| Modded content | Wire representation for vanilla clients |
|---|---|
| Entity `examplemod:ruby_golem` | spawn as nearest-size vanilla type (e.g. `iron_golem`), metadata rewrite: custom name, health, scale |
| Block `examplemod:ruby_ore` | vanilla block with matching shape/category + block-entity payload carrying the modded key; client renders per table (M4: resource pack injection for real textures) |
| Item `examplemod:ruby` | vanilla item + custom data component embedding the modded key + display name |
| Custom menu | nearest vanilla menu type (chest/hopper/…) |
| Sounds/particles | mapped via registry tables; fallback to vanilla analog |

Key properties:

1. **Deterministic mapping** — `imagine-registry` `MappingTable` +
   `imagine-protocol` `Translator` decide once at registration time, then
   the hot path is table lookups only (O(1), no parsing per packet).
2. **Server-authoritative** — the client is never asked to understand
   modded content; it receives 100% vanilla 26.3 protocol.
3. **Failure mode = vanilla analog**, never a broken packet (a broken
   packet disconnects clients; a wrong-but-valid analog just looks plain).

## Known issues inherited from F2V (to fix in M4)

The original F2V had these confirmed visual bugs; each becomes an
acceptance test in `visual.yml`:

- entity metadata written with 1.21.1 indices (off-by-N on 26.3) →
  glowing/garbled skins;
- block entity NBT sent before the vanilla block was accepted → invisible
  blocks until chunk reload;
- item components serialized in 1.21.1 order → client kick on some items;
- spawn packet position rounding broke yaw/pitch for entities >32 blocks.

## CI methodology

`visual.yml` (manual/nightly):

1. boot Paper+imagine with the test modpack (xvfb);
2. connect a headless vanilla 26.3 client (liberica/temurin + stock jar,
   automated with a small driver);
3. teleport through a gallery world containing every test content item;
4. screenshot → compare against reference captures (SSIM threshold) and
   against a real NeoForge 1.21.1 client capture (semantic check list);
5. upload all captures as artifacts; failures annotate the diff image.
