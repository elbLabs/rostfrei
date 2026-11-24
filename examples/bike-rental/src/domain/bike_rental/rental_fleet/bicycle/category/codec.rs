use super::BicycleCategory;

impl TryFrom<String> for BicycleCategory {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value).ok_or("bicycle category must be non-empty and trimmed")
    }
}
