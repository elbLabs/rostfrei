use super::DecisionOutputDescriptor;

pub trait DecisionOutputType: 'static {
    const DESCRIPTOR: DecisionOutputDescriptor;
}
