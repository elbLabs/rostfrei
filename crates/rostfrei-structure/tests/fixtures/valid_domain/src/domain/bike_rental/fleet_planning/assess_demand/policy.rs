#[domain_policy(id = "assess-demand", label = "Assess demand")]
pub trait AssessDemandPolicy {
    fn assess_demand(input: DemandForecast) -> usize;
}
