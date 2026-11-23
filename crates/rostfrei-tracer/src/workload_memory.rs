//! Read-only memory samples for explicitly mapped read-model workloads.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, symlink_metadata},
    io::Read as _,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use serde::Serialize;
use tokio::sync::Semaphore;

/// A transport-neutral source. Labels are public identifiers, never paths.
/// Missing labels are reported as unavailable, and extra results are ignored.
#[async_trait::async_trait]
pub trait WorkloadMemorySource: Send + Sync {
    async fn sample(&self, labels: &[String]) -> BTreeMap<String, WorkloadMemory>;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadMemory {
    /// Memory charged to the container workload, not to individual projections.
    pub current_bytes: Option<u64>,
    pub limit: WorkloadMemoryLimit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum WorkloadMemoryLimit {
    Limited {
        bytes: u64,
    },
    Unlimited,
    #[default]
    Unavailable,
}

#[derive(Debug, Serialize)]
pub struct WorkloadMemorySnapshot {
    pub workloads: Vec<WorkloadMemorySample>,
}

#[derive(Debug, Serialize)]
pub struct WorkloadMemorySample {
    pub label: String,
    pub memory: WorkloadMemory,
}

const MAX_WORKLOADS: usize = 32;
const MAX_FILE_BYTES: u8 = 64;

/// Reads two files in each explicitly supplied Linux cgroup-v2 directory.
/// Directories must be trusted operator configuration, never request input.
pub struct CgroupV2Workloads {
    directories: BTreeMap<String, PathBuf>,
    in_flight: Arc<Semaphore>,
}

/// Configuration errors deliberately omit operator-supplied paths.
#[derive(Debug, thiserror::Error)]
pub enum CgroupV2WorkloadConfigError {
    #[error("workload labels must be nonempty and cannot contain path separators")]
    InvalidLabel,
    #[error("workload labels must be unique")]
    DuplicateLabel,
    #[error("each cgroup-v2 directory must be an absolute path")]
    RelativeDirectory,
    #[error("at most {MAX_WORKLOADS} cgroup-v2 workloads can be configured")]
    TooManyWorkloads,
}

impl CgroupV2Workloads {
    pub fn new(
        sources: impl IntoIterator<Item = (impl Into<String>, impl Into<PathBuf>)>,
    ) -> Result<Self, CgroupV2WorkloadConfigError> {
        let mut directories = BTreeMap::new();
        for (label, directory) in sources {
            let label = label.into();
            if label.trim().is_empty() || label.contains(['/', '\\']) {
                return Err(CgroupV2WorkloadConfigError::InvalidLabel);
            }
            let directory = directory.into();
            if !directory.is_absolute() {
                return Err(CgroupV2WorkloadConfigError::RelativeDirectory);
            }
            if directories.insert(label, directory).is_some() {
                return Err(CgroupV2WorkloadConfigError::DuplicateLabel);
            }
            if directories.len() > MAX_WORKLOADS {
                return Err(CgroupV2WorkloadConfigError::TooManyWorkloads);
            }
        }
        Ok(Self {
            directories,
            in_flight: Arc::new(Semaphore::new(1)),
        })
    }
}

#[async_trait::async_trait]
impl WorkloadMemorySource for CgroupV2Workloads {
    async fn sample(&self, labels: &[String]) -> BTreeMap<String, WorkloadMemory> {
        let Ok(permit) = Arc::clone(&self.in_flight).try_acquire_owned() else {
            return BTreeMap::new();
        };
        let sources = labels
            .iter()
            .take(MAX_WORKLOADS)
            .filter_map(|label| {
                self.directories
                    .get(label)
                    .map(|directory| (label.clone(), directory.clone()))
            })
            .collect::<Vec<_>>();
        tokio::task::spawn_blocking(move || {
            // Keep the permit until the actual file reads finish, even on timeout.
            let _permit = permit;
            sources
                .into_iter()
                .map(|(label, directory)| {
                    let current_bytes = read_value(&directory.join("memory.current"))
                        .and_then(|text| text.trim().parse::<u64>().ok());
                    let limit = match read_value(&directory.join("memory.max"))
                        .as_deref()
                        .map(str::trim)
                    {
                        Some("max") => WorkloadMemoryLimit::Unlimited,
                        Some(number) => number
                            .parse::<u64>()
                            .ok()
                            .map_or(WorkloadMemoryLimit::Unavailable, |bytes| {
                                WorkloadMemoryLimit::Limited { bytes }
                            }),
                        None => WorkloadMemoryLimit::Unavailable,
                    };
                    (
                        label,
                        WorkloadMemory {
                            current_bytes,
                            limit,
                        },
                    )
                })
                .collect()
        })
        .await
        .unwrap_or_default()
    }
}

fn read_value(path: &Path) -> Option<String> {
    if !symlink_metadata(path).ok()?.is_file() {
        return None;
    }
    let mut value = String::new();
    File::open(path)
        .ok()?
        .take(u64::from(MAX_FILE_BYTES).saturating_add(1))
        .read_to_string(&mut value)
        .ok()?;
    (value.len() <= usize::from(MAX_FILE_BYTES)).then_some(value)
}

pub(crate) struct WorkloadInspection {
    source: Arc<dyn WorkloadMemorySource>,
    labels: Vec<String>,
    in_flight: Semaphore,
}

impl WorkloadInspection {
    pub(crate) fn new(
        source: Arc<dyn WorkloadMemorySource>,
        inventory: Option<&crate::StudioDomainInventory>,
    ) -> Option<Self> {
        let mut labels = BTreeSet::new();
        for label in inventory?
            .read_models
            .iter()
            .filter_map(|model| model.workload.as_ref())
        {
            labels.insert(label.clone());
            if labels.len() == MAX_WORKLOADS {
                break;
            }
        }
        (!labels.is_empty()).then(|| Self {
            source,
            labels: labels.into_iter().collect(),
            in_flight: Semaphore::new(1),
        })
    }

    pub(crate) async fn snapshot(&self) -> WorkloadMemorySnapshot {
        let mut sampled = if let Ok(_permit) = self.in_flight.try_acquire() {
            tokio::time::timeout(Duration::from_secs(2), self.source.sample(&self.labels))
                .await
                .unwrap_or_default()
        } else {
            BTreeMap::new()
        };
        WorkloadMemorySnapshot {
            workloads: self
                .labels
                .iter()
                .map(|label| WorkloadMemorySample {
                    label: label.clone(),
                    memory: sampled.remove(label).unwrap_or_default(),
                })
                .collect(),
        }
    }
}
