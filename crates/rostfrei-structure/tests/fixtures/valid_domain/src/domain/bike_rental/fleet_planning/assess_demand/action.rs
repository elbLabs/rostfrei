#[domain_action(id = "assess-demand", label = "Assess demand")]
pub trait AssessDemand {
    fn assess_demand(input: DemandForecast);
}
