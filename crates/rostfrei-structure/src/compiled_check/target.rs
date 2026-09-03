use serde::Deserialize;

pub(super) const DOMAIN_CHECK_TARGET: &str = "rostfrei-domain-check";

#[derive(Clone, Debug, Deserialize)]
pub struct CargoTarget {
    name: String,
    kind: Vec<String>,
}

pub(super) fn has_domain_check_target(targets: &[CargoTarget]) -> bool {
    targets.iter().any(|target| {
        target.name == DOMAIN_CHECK_TARGET && target.kind.iter().any(|kind| kind == "bin")
    })
}

#[cfg(test)]
mod tests {
    use super::{CargoTarget, has_domain_check_target};

    fn target(name: &str, kind: &[&str]) -> CargoTarget {
        CargoTarget {
            name: name.to_owned(),
            kind: kind.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    #[test]
    fn finds_the_conventional_binary_target() {
        let targets = [target("rostfrei-domain-check", &["bin"])];

        assert!(has_domain_check_target(&targets));
    }

    #[test]
    fn ignores_a_non_binary_target_with_the_conventional_name() {
        let targets = [target("rostfrei-domain-check", &["lib"])];

        assert!(!has_domain_check_target(&targets));
    }

    #[test]
    fn ignores_other_binary_targets() {
        let targets = [target("bike-rental-api", &["bin"])];

        assert!(!has_domain_check_target(&targets));
    }
}
