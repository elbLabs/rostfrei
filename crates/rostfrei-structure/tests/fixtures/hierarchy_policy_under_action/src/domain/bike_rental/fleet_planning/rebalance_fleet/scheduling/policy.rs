#[domain_policy(id = "scheduling", label = "Scheduling")]
pub trait SchedulingPolicy {
    fn schedule();
}
