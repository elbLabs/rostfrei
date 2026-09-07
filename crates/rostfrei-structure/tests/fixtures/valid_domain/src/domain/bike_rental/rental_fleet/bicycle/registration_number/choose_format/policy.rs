#[domain_policy(id = "choose-registration-format", label = "Choose registration format")]
pub trait ChooseRegistrationFormat {
    fn choose_format(&self) -> RegistrationFormat;
}
