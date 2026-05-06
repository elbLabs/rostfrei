#[domain_action(id = "rebalance-fleet", label = "Rebalance fleet")]
pub trait RebalanceFleetAction {
    fn rebalance_fleet();
}
