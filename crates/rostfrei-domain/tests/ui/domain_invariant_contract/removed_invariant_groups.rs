#![allow(unused, non_snake_case)]

use rostfrei_domain::domain_model;

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
        query_groups: [],
        invariant_groups: [],
    };
}
