use serde::Serialize;
use std::sync::{Arc, Mutex};

/// A small, explicit capability seam used by the harness core. Capabilities
/// register their public name once at startup; consumers use the registry for
/// diagnostics without coupling the UI to implementation modules.
pub trait Capability: Send + Sync {
    fn name(&self) -> &'static str;
    fn register(&self, registry: &mut CapabilityRegistry);
}

#[derive(Clone, Default)]
pub struct CapabilityRegistry {
    names: Arc<Mutex<Vec<String>>>,
}

impl CapabilityRegistry {
    pub fn register_capability(&mut self, capability: &dyn Capability) {
        capability.register(self);
    }

    pub fn add(&mut self, name: &'static str) {
        if let Ok(mut names) = self.names.lock() {
            if !names.iter().any(|current| current == name) {
                names.push(name.to_string());
                names.sort();
            }
        }
    }

    pub fn names(&self) -> Vec<String> {
        self.names
            .lock()
            .map(|names| names.clone())
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy)]
pub struct NamedCapability(pub &'static str);

impl Capability for NamedCapability {
    fn name(&self) -> &'static str {
        self.0
    }

    fn register(&self, registry: &mut CapabilityRegistry) {
        registry.add(self.name());
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CapabilityStatus {
    pub name: String,
    pub enabled: bool,
}

pub fn default_registry() -> CapabilityRegistry {
    let mut registry = CapabilityRegistry::default();
    for name in [
        "provider-runtime",
        "session-events",
        "tool-execution",
        "workspace-discovery",
        "refactor-planning",
        "context-compaction",
        "security-approvals",
        "resource-telemetry",
        "workspace-deliverables",
        "safe-browser",
        "web-research",
    ] {
        registry.register_capability(&NamedCapability(name));
    }
    registry
}
