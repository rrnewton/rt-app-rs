//! Parsing of the `"resources"` section and auto-creation of resources
//! referenced by events.
//!
//! Resources in rt-app are named synchronisation primitives (mutexes, timers,
//! condition variables, barriers, memory buffers, I/O devices) that events
//! operate on. They can be explicitly declared in the `"resources"` JSON
//! section, or auto-created when an event references a resource name that
//! does not yet exist.

use std::collections::HashMap;

use serde::Deserialize;

use crate::types::{ResourceData, ResourceIndex, ResourceType};

use super::ConfigError;

// ---------------------------------------------------------------------------
// Parsed resource declaration
// ---------------------------------------------------------------------------

/// A single resource entry parsed from the configuration.
#[derive(Debug, Clone)]
pub struct ResourceConfig {
    /// Name of the resource (the JSON key).
    pub name: String,
    /// Index assigned during parsing.
    pub index: ResourceIndex,
    /// The kind of resource.
    pub resource_type: ResourceType,
}

// ---------------------------------------------------------------------------
// Resource table: manages resource registration and lookup
// ---------------------------------------------------------------------------

/// Manages all resources (both explicitly declared and auto-created).
#[derive(Debug, Clone)]
pub struct ResourceTable {
    resources: Vec<ResourceConfig>,
    /// Maps (name, type) pairs to indices for fast lookup.
    index_map: HashMap<(String, ResourceType), ResourceIndex>,
}

impl ResourceTable {
    pub fn new() -> Self {
        Self {
            resources: Vec::new(),
            index_map: HashMap::new(),
        }
    }

    /// Number of registered resources.
    pub fn len(&self) -> usize {
        self.resources.len()
    }

    /// Whether the table contains no resources.
    pub fn is_empty(&self) -> bool {
        self.resources.is_empty()
    }

    /// Iterator over all resource configs.
    pub fn iter(&self) -> impl Iterator<Item = &ResourceConfig> {
        self.resources.iter()
    }

    /// Look up or auto-create a resource by name and type.
    /// Returns the resource index.
    pub fn get_or_create(&mut self, name: &str, rtype: ResourceType) -> ResourceIndex {
        let key = (name.to_owned(), rtype);
        if let Some(&idx) = self.index_map.get(&key) {
            return idx;
        }
        let idx = ResourceIndex(self.resources.len());
        self.resources.push(ResourceConfig {
            name: name.to_owned(),
            index: idx,
            resource_type: rtype,
        });
        self.index_map.insert(key, idx);
        idx
    }

    /// Look up a resource by index.
    pub fn get(&self, idx: ResourceIndex) -> Option<&ResourceConfig> {
        self.resources.get(idx.0)
    }

    /// Convert a `ResourceConfig` into the runtime `ResourceData` for its type.
    pub fn make_resource_data(rtype: ResourceType) -> ResourceData {
        match rtype {
            ResourceType::Mutex | ResourceType::Lock | ResourceType::Unlock => ResourceData::Mutex,
            ResourceType::Wait
            | ResourceType::Signal
            | ResourceType::Broadcast
            | ResourceType::SigAndWait
            | ResourceType::Suspend
            | ResourceType::Resume => ResourceData::Condition,
            ResourceType::Timer | ResourceType::TimerUnique => ResourceData::Timer {
                next: None,
                initialized: false,
                relative: true,
            },
            ResourceType::Barrier => ResourceData::Barrier { waiting: 0 },
            ResourceType::Mem | ResourceType::IoRun => ResourceData::IoMem { size: 0 },
            ResourceType::Fork => ResourceData::Fork {
                reference: String::new(),
                num_forks: 0,
            },
            _ => ResourceData::Mutex, // fallback
        }
    }
}

impl Default for ResourceTable {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// JSON resource type string
// ---------------------------------------------------------------------------

/// Map the JSON `"type"` string of a resource declaration to a `ResourceType`.
fn resource_type_from_str(s: &str) -> Result<ResourceType, ConfigError> {
    match s {
        "mutex" => Ok(ResourceType::Mutex),
        "timer" => Ok(ResourceType::Timer),
        "cond" | "wait" => Ok(ResourceType::Wait),
        "barrier" => Ok(ResourceType::Barrier),
        "membuf" | "mem" => Ok(ResourceType::Mem),
        "iodev" | "iorun" => Ok(ResourceType::IoRun),
        _ => Err(ConfigError::InvalidResourceType(s.to_owned())),
    }
}

/// Intermediate struct for a declared resource in the JSON `"resources"` object.
#[derive(Debug, Deserialize)]
struct RawResource {
    #[serde(rename = "type", default = "default_resource_type")]
    resource_type: String,
}

fn default_resource_type() -> String {
    "mutex".to_owned()
}

/// Parse the `"resources"` section from a JSON value into a `ResourceTable`.
pub fn parse_resources(value: Option<&serde_json::Value>) -> Result<ResourceTable, ConfigError> {
    let mut table = ResourceTable::new();

    let obj = match value {
        Some(serde_json::Value::Object(map)) => map,
        Some(serde_json::Value::Null) | None => return Ok(table),
        Some(_) => return Err(ConfigError::InvalidSection("resources")),
    };

    for (name, val) in obj {
        let raw: RawResource = serde_json::from_value(val.clone()).map_err(ConfigError::Json)?;
        let rtype = resource_type_from_str(&raw.resource_type)?;
        table.get_or_create(name, rtype);
    }

    Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_resources() {
        let table = parse_resources(None).unwrap();
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn parse_explicit_resources() {
        let json = serde_json::json!({
            "mutex1": {"type": "mutex"},
            "timer1": {"type": "timer"},
            "barrier1": {"type": "barrier"}
        });
        let table = parse_resources(Some(&json)).unwrap();
        assert_eq!(table.len(), 3);
    }

    #[test]
    fn auto_create_resource() {
        let mut table = ResourceTable::new();
        let idx1 = table.get_or_create("my_mutex", ResourceType::Mutex);
        let idx2 = table.get_or_create("my_mutex", ResourceType::Mutex);
        assert_eq!(idx1, idx2);
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn same_name_different_type() {
        let mut table = ResourceTable::new();
        let idx1 = table.get_or_create("res", ResourceType::Mutex);
        let idx2 = table.get_or_create("res", ResourceType::Wait);
        assert_ne!(idx1, idx2);
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn default_type_is_mutex() {
        let json = serde_json::json!({
            "res1": {}
        });
        let table = parse_resources(Some(&json)).unwrap();
        let r = table.get(ResourceIndex(0)).unwrap();
        assert_eq!(r.resource_type, ResourceType::Mutex);
    }

    #[test]
    fn invalid_resource_type() {
        let json = serde_json::json!({
            "res1": {"type": "magic"}
        });
        assert!(parse_resources(Some(&json)).is_err());
    }

    #[test]
    fn resource_data_for_types() {
        assert!(matches!(
            ResourceTable::make_resource_data(ResourceType::Mutex),
            ResourceData::Mutex
        ));
        assert!(matches!(
            ResourceTable::make_resource_data(ResourceType::Timer),
            ResourceData::Timer { .. }
        ));
        assert!(matches!(
            ResourceTable::make_resource_data(ResourceType::Barrier),
            ResourceData::Barrier { .. }
        ));
        assert!(matches!(
            ResourceTable::make_resource_data(ResourceType::Wait),
            ResourceData::Condition
        ));
    }
}
