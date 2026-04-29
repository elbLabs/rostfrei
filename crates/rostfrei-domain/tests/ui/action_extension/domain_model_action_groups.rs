use domain::domain_model;

fn main() {
    let _ = domain_model! {
        contexts: [],
        aggregates: [],
        entities: [],
        value_objects: [],
        services: [],
        errors: [],
        action_groups: [],
        query_groups: [],
    };
}
