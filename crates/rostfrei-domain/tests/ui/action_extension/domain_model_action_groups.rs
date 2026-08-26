use domain::domain_model;

fn main() {
    let _ = domain_model! {
        contexts: [],
        aggregates: [],
        entities: [],
        identities: [],
        value_objects: [],
        services: [],
        commands: [],
        events: [],
        errors: [],
        action_groups: [],
        query_groups: [],
    };
}
