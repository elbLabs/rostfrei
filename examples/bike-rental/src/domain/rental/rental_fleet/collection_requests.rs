use std::collections::HashMap;

use crate::domain::rental_payment::CollectionPayment;

pub(super) type CollectionRequests = HashMap<String, CollectionPayment>;
