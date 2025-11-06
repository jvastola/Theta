use once_cell::sync::Lazy;
use serde::Serialize;
use siphasher::sip::SipHasher24;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComponentManifestEntry {
    pub type_name: &'static str,
    pub stable_hash: u64,
}

impl ComponentManifestEntry {
    pub fn new(type_name: &'static str) -> Self {
        Self {
            type_name,
            stable_hash: stable_component_hash(type_name),
        }
    }

    pub fn of<T: 'static>() -> Self {
        Self::new(std::any::type_name::<T>())
    }
}

static REGISTRY: Lazy<Mutex<Vec<ComponentManifestEntry>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Registers a component manifest entry. Duplicate registrations are ignored.
pub fn register_entry(entry: ComponentManifestEntry) {
    let mut guard = REGISTRY.lock().expect("component registry mutex poisoned");

    if guard
        .iter()
        .any(|existing| existing.type_name == entry.type_name)
    {
        return;
    }

    guard.push(entry);
}

/// Returns a sorted snapshot of registered component entries.
pub fn registered_entries() -> Vec<ComponentManifestEntry> {
    let guard = REGISTRY.lock().expect("component registry mutex poisoned");
    let mut entries = guard.clone();
    entries.sort_by(|a, b| a.type_name.cmp(b.type_name));
    entries
}

/// Writes the manifest to the provided JSON file path.
pub fn write_manifest_json(path: &Path) -> std::io::Result<()> {
    let entries = registered_entries();
    let manifest = ComponentManifest {
        components: entries,
    };
    let json = serde_json::to_vec_pretty(&manifest).expect("manifest serialization should succeed");
    std::fs::create_dir_all(path.parent().unwrap_or_else(|| Path::new(".")))?;
    std::fs::write(path, json)
}

#[derive(Debug, Serialize)]
struct ComponentManifest {
    components: Vec<ComponentManifestEntry>,
}

/// Computes the stable SipHash-2-4 identifier for the provided type name.
pub fn stable_component_hash(type_name: &str) -> u64 {
    let mut hasher = SipHasher24::new_with_keys(STABLE_HASH_KEY_0, STABLE_HASH_KEY_1);
    type_name.hash(&mut hasher);
    hasher.finish()
}

const STABLE_HASH_KEY_0: u64 = 0x0ddcc001feedface;
const STABLE_HASH_KEY_1: u64 = 0xabcdef0123456789;

/// Macro to register a component type for the manifest. Expands to a `ctor`
/// that runs during program initialization.
#[macro_export]
macro_rules! register_component_types {
    ($($ty:ty),+ $(,)?) => {
        #[ctor::ctor]
        fn __theta_register_components() {
            $(
                $crate::network::schema::register_entry(
                    $crate::network::schema::ComponentManifestEntry::of::<$ty>(),
                );
            )+
        }
    };
}

#[macro_export]
macro_rules! register_component_type {
    ($ty:ty) => {
        $crate::register_component_types!($ty);
    };
}

/// Validates that all registered component hashes are unique.
pub fn assert_no_hash_collisions() {
    let entries = registered_entries();
    let mut hashes = HashSet::new();
    for entry in entries {
        if !hashes.insert(entry.stable_hash) {
            panic!("detected hash collision for type {}", entry.type_name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Position;
    struct Velocity;

    register_component_types!(Position, Velocity);

    #[test]
    fn stable_hash_is_deterministic() {
        let hash_a = stable_component_hash("example::Position");
        let hash_b = stable_component_hash("example::Position");
        assert_eq!(hash_a, hash_b);
    }

    #[test]
    fn registry_collects_unique_entries() {
        let entries = registered_entries();
        assert!(entries.len() >= 2);
        let mut hashes = entries
            .iter()
            .map(|entry| entry.stable_hash)
            .collect::<Vec<_>>();
        hashes.sort();
        let before = hashes.len();
        hashes.dedup();
        assert_eq!(before, hashes.len());
    }

    #[test]
    fn manifest_writes_to_disk() {
        let entries = registered_entries();
        assert!(
            entries
                .iter()
                .any(|entry| entry.type_name.ends_with("Position"))
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry.type_name.ends_with("Velocity"))
        );
        let tmp_dir = tempfile::tempdir().expect("tmpdir");
        let path = tmp_dir.path().join("manifest.json");
        write_manifest_json(&path).expect("write manifest");
        let contents = std::fs::read_to_string(path).expect("read manifest");
        assert!(contents.contains("Position"));
        assert!(contents.contains("Velocity"));
    }
}
