use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use async_trait::async_trait;
use rostfrei::{
    BoundedContext, ReadModel, ReadModelBackend, ReadModelEntry, ReadModelError,
    ReadModelErrorKind, ReadModelKey, ReadModelRevision, ReadModelState, ReadModelStore,
    ReadModels,
};
use rostfrei_core::EventHistory;

use super::{
    BillingChanged, DeliveryDisposition, Fixture, IntegrationEventOrder, NatsReadModelBackend,
    OrganizationAccess, ReadModelProcessingError, TestResult, deliver, register_business,
};

#[derive(Default)]
pub struct Faults {
    conflicts: AtomicU32,
    pub(super) ambiguous: AtomicBool,
    pub(super) unavailable_once: AtomicBool,
    unavailable: AtomicBool,
    pub(super) committed: AtomicU32,
}

pub struct Backend {
    pub(super) inner: NatsReadModelBackend,
    pub(super) faults: Arc<Faults>,
}

#[async_trait]
impl ReadModelBackend for Backend {
    async fn open<M: ReadModel>(
        &self,
        context: &BoundedContext,
    ) -> Result<Arc<dyn ReadModelStore<ReadModelState<M>>>, ReadModelError> {
        Ok(Arc::new(Store {
            inner: self.inner.open::<M>(context).await?,
            faults: self.faults.clone(),
        }))
    }
    async fn history(
        &self,
        context: &BoundedContext,
    ) -> Result<Arc<dyn EventHistory>, ReadModelError> {
        self.inner.history(context).await
    }
}

struct Store<M: ReadModel> {
    inner: Arc<dyn ReadModelStore<ReadModelState<M>>>,
    faults: Arc<Faults>,
}

impl<M: ReadModel> Store<M> {
    fn before(&self) -> Result<(), ReadModelError> {
        if self.faults.unavailable.load(Ordering::SeqCst)
            || self.faults.unavailable_once.swap(false, Ordering::SeqCst)
        {
            return Err(ReadModelError::new(
                ReadModelErrorKind::Unavailable,
                "injected outage",
            ));
        }
        if self
            .faults
            .conflicts
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err(ReadModelError::new(
                ReadModelErrorKind::Conflict,
                "injected competing writer",
            ));
        }
        Ok(())
    }
    fn after(&self, revision: ReadModelRevision) -> Result<ReadModelRevision, ReadModelError> {
        self.faults.committed.fetch_add(1, Ordering::SeqCst);
        if self.faults.ambiguous.swap(false, Ordering::SeqCst) {
            return Err(ReadModelError::new(
                ReadModelErrorKind::Unavailable,
                "lost persistence acknowledgement",
            ));
        }
        Ok(revision)
    }
}

#[async_trait]
impl<M: ReadModel> ReadModelStore<ReadModelState<M>> for Store<M> {
    async fn read(
        &self,
        key: &ReadModelKey,
    ) -> Result<Option<ReadModelEntry<ReadModelState<M>>>, ReadModelError> {
        self.inner.read(key).await
    }
    async fn create(
        &self,
        key: &ReadModelKey,
        value: &ReadModelState<M>,
    ) -> Result<ReadModelRevision, ReadModelError> {
        self.before()?;
        self.after(self.inner.create(key, value).await?)
    }
    async fn update(
        &self,
        key: &ReadModelKey,
        revision: &ReadModelRevision,
        value: &ReadModelState<M>,
    ) -> Result<ReadModelRevision, ReadModelError> {
        self.before()?;
        self.after(self.inner.update(key, revision, value).await?)
    }
    async fn delete(
        &self,
        key: &ReadModelKey,
        revision: &ReadModelRevision,
    ) -> Result<(), ReadModelError> {
        self.inner.delete(key, revision).await
    }
}

#[tokio::test]
async fn conflicts_are_bounded_and_uncertain_commits_are_not_applied_twice() -> TestResult {
    let fixture = Fixture::new().await?;
    let faults = Arc::new(Faults::default());
    let models = ReadModels::new(
        fixture.context.clone(),
        Backend {
            inner: fixture.backend(),
            faults: faults.clone(),
        },
    );
    let model = register_business(&models, fixture.billing.clone()).await?;
    let key = ReadModelKey::new("org-1")?;
    faults.conflicts.store(16, Ordering::SeqCst);
    assert!(matches!(
        deliver(&model, "contended", 7, true).await?,
        DeliveryDisposition::RetryAfter(_)
    ));
    assert_eq!(faults.conflicts.load(Ordering::SeqCst), 8);
    assert!(model.reader().read(&key).await?.is_none());
    faults.conflicts.store(2, Ordering::SeqCst);
    faults.ambiguous.store(true, Ordering::SeqCst);
    assert!(matches!(
        deliver(&model, "uncertain", 7, true).await?,
        DeliveryDisposition::RetryAfter(_)
    ));
    assert!(
        model
            .reader()
            .read(&key)
            .await?
            .ok_or("missing committed value")?
            .paid
    );
    assert_eq!(
        deliver(&model, "redelivered", 7, true).await?,
        DeliveryDisposition::Acknowledge
    );
    assert_eq!(faults.committed.load(Ordering::SeqCst), 1);
    faults.unavailable.store(true, Ordering::SeqCst);
    assert!(matches!(
        deliver(&model, "outage", 8, false).await?,
        DeliveryDisposition::RetryAfter(_)
    ));
    assert!(model.reader().read(&key).await?.ok_or("missing")?.paid);
    faults.unavailable.store(false, Ordering::SeqCst);
    assert_eq!(
        deliver(&model, "repaired", 8, false).await?,
        DeliveryDisposition::Acknowledge
    );
    assert!(!model.reader().read(&key).await?.ok_or("missing")?.paid);
    fixture.cleanup().await
}

#[tokio::test]
async fn failed_transformations_do_not_save_partial_values_or_checkpoints() -> TestResult {
    let fixture = Fixture::new().await?;
    let models = ReadModels::new(fixture.context.clone(), fixture.backend());
    let failure = Arc::new(AtomicBool::new(true));
    let fail = failure.clone();
    let model = models
        .register::<OrganizationAccess>()
        .try_on_integration_event::<BillingChanged>(
            |event| event.organization_id.clone(),
            move |view, event| {
                view.paid = event.paid;
                if fail.load(Ordering::SeqCst) {
                    return Err(ReadModelProcessingError::Transformation(
                        "cannot calculate view".to_owned(),
                    ));
                }
                Ok(())
            },
        )
        .integration_source::<BillingChanged>(
            fixture.billing.clone(),
            IntegrationEventOrder::<BillingChanged>::latest("billing", |event| {
                event.source_version
            }),
        )
        .build()
        .await?;
    assert!(matches!(
        deliver(&model, "failure", 7, true).await?,
        DeliveryDisposition::Quarantine(_)
    ));
    let key = ReadModelKey::new("org-1")?;
    assert!(model.reader().read(&key).await?.is_none());
    failure.store(false, Ordering::SeqCst);
    assert_eq!(
        deliver(&model, "replay", 7, true).await?,
        DeliveryDisposition::Acknowledge
    );
    assert!(model.reader().read(&key).await?.ok_or("missing")?.paid);
    fixture.cleanup().await
}
