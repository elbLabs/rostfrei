use rostfrei::domain_model;

use super::{{context_type}};

pub fn domain_model() -> Result<serde_json::Value, rostfrei::DomainModelError> {
    domain_model! {
        contexts: [{{context_type}}],
        aggregates: [],
        entities: [],
        value_objects: [],
        services: [],
        errors: [],
    }
}
