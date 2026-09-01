#![allow(dead_code)]

use domain::{
    ActionInputDescriptor, ActionOutputDescriptor, DomainError, DomainErrorType, DomainIdentity,
    Entity, EntityType, ScalarType, ValueObject, ValueObjectType, domain_actions, domain_model,
};

pub mod contracts {
    use domain::domain_actions;

    use super::Title;

    #[domain_actions(entity)]
    pub(super) trait TaskWorkflow {
        #[action(id = "rename", label = "Rename task")]
        fn rename(&mut self, input: Title) -> Title;

        #[action(id = "complete", label = "Complete task")]
        fn complete(&mut self);

        #[action(id = "reject", label = "Reject task")]
        fn reject(&mut self) -> Result<(), super::TaskRejected>;
    }
}

#[domain_actions(entity)]
trait TaskInspection {
    #[action(id = "is-complete", label = "Is task complete")]
    fn is_complete(&self) -> bool;

    #[action(id = "title", label = "Task title")]
    fn title(&self) -> Title;

    #[action(id = "title-history", label = "Task title history")]
    fn title_history(&self) -> Option<Vec<Title>>;
}

#[domain_actions(entity)]
trait UnlistedTaskActions {
    #[action(id = "revision", label = "Task revision")]
    fn revision(&self) -> u32;
}

#[derive(domain::BoundedContext)]
#[domain(id = "planning", label = "Planning")]
struct Planning;

#[derive(DomainIdentity)]
#[domain(owner = Task)]
struct TaskId(u64);

#[derive(Entity)]
#[domain(
    id = "task",
    label = "Task",
    owner = Project,
    actions = [TaskInspection, contracts::TaskWorkflow]
)]
struct Task {
    #[domain(identity)]
    id: TaskId,
    title: String,
    complete: bool,
    revision: u32,
}

#[derive(domain::Aggregate)]
#[domain(id = "project", label = "Project")]
struct Project;

impl domain::AggregateDefinition for Project {
    type Context = Planning;
    type Root = Task;
    type Event = domain::NoDomainEvents;
}

#[derive(ValueObject, Debug, Eq, PartialEq)]
#[domain(id = "title", label = "Title", owner = Task)]
struct Title(String);

#[derive(DomainError, Debug, Eq, PartialEq)]
#[domain(
    id = "task-rejected",
    label = "Task rejected",
    owner = Task,
    code = "TASK_REJECTED",
    message = "The task change was rejected."
)]
struct TaskRejected;

impl TaskInspection for Task {
    fn is_complete(&self) -> bool {
        self.complete
    }

    fn title(&self) -> Title {
        Title(self.title.clone())
    }

    fn title_history(&self) -> Option<Vec<Title>> {
        Some(vec![self.title()])
    }
}

impl contracts::TaskWorkflow for Task {
    fn rename(&mut self, input: Title) -> Title {
        self.title = input.0;
        self.revision = self.revision.saturating_add(1);
        Title(self.title.clone())
    }

    fn complete(&mut self) {
        self.complete = true;
        self.revision = self.revision.saturating_add(1);
    }

    fn reject(&mut self) -> Result<(), TaskRejected> {
        Err(TaskRejected)
    }
}

impl UnlistedTaskActions for Task {
    fn revision(&self) -> u32 {
        self.revision
    }
}

#[derive(DomainIdentity)]
#[domain(owner = Comment)]
struct CommentId(u64);

#[derive(Entity)]
#[domain(id = "comment", label = "Comment", owner = Project, actions = [])]
struct Comment {
    #[domain(identity)]
    id: CommentId,
    body: String,
}

fn task() -> Task {
    Task {
        id: TaskId(7),
        title: "Draft".to_owned(),
        complete: false,
        revision: 0,
    }
}

#[test]
fn ordinary_trait_implementations_run_with_supported_receivers_and_arities() {
    use contracts::TaskWorkflow as _;

    let mut task = task();
    assert!(!task.is_complete());
    assert_eq!(task.title(), Title("Draft".to_owned()));
    assert_eq!(task.title_history(), Some(vec![Title("Draft".to_owned())]));
    assert_eq!(task.revision(), 0);

    assert_eq!(
        task.rename(Title("Ready".to_owned())),
        Title("Ready".to_owned())
    );
    assert_eq!(task.reject(), Err(TaskRejected));
    task.complete();

    assert!(task.is_complete());
    assert_eq!(task.title(), Title("Ready".to_owned()));
    assert_eq!(task.revision(), 2);
}

#[test]
fn entity_action_contracts_preserve_list_and_trait_source_order() {
    let contracts = <Task as EntityType>::ACTION_CONTRACTS;
    assert_eq!(contracts.len(), 2);
    assert_eq!(
        contracts[0]
            .iter()
            .map(|action| action.id.local)
            .collect::<Vec<_>>(),
        ["is-complete", "title", "title-history"]
    );
    assert_eq!(
        contracts[1]
            .iter()
            .map(|action| action.id.local)
            .collect::<Vec<_>>(),
        ["rename", "complete", "reject"]
    );

    assert_eq!(contracts[0][0].input, None);
    assert_eq!(
        contracts[0][0].output,
        Some(ActionOutputDescriptor::Scalar(ScalarType::Bool))
    );
    assert_eq!(
        contracts[0][1].output,
        Some(ActionOutputDescriptor::ValueObject(Title::DESCRIPTOR.id))
    );
    assert_eq!(
        contracts[0][2].output,
        Some(ActionOutputDescriptor::Optional(
            &ActionOutputDescriptor::List(&ActionOutputDescriptor::ValueObject(
                Title::DESCRIPTOR.id,
            )),
        ))
    );
    assert_eq!(
        contracts[1][0].input,
        Some(ActionInputDescriptor::ValueObject(Title::DESCRIPTOR.id))
    );
    assert_eq!(
        contracts[1][0].output,
        Some(ActionOutputDescriptor::ValueObject(Title::DESCRIPTOR.id))
    );
    assert_eq!(contracts[1][0].error, None);
    assert_eq!(contracts[1][1].input, None);
    assert_eq!(contracts[1][1].output, None);
    assert_eq!(contracts[1][2].error, Some(TaskRejected::DESCRIPTOR.id));

    let unlisted = <Task as UnlistedTaskActions>::__DOMAIN_ACTIONS;
    assert_eq!(
        unlisted
            .iter()
            .map(|action| action.id.local)
            .collect::<Vec<_>>(),
        ["revision"]
    );

    assert!(<Comment as EntityType>::ACTION_CONTRACTS.is_empty());
}

#[test]
fn domain_model_automatically_projects_only_listed_entity_action_traits() {
    let model = domain_model! {
        contexts: [Planning],
        aggregates: [Project],
        entities: [Task, Comment],
        identities: [TaskId, CommentId],
        value_objects: [Title],
        services: [],
        commands: [],
        errors: [TaskRejected],
        query_groups: [],
    }
    .expect("entity action domain model should be valid");

    let actions = model["actions"].as_array().unwrap();
    assert_eq!(
        actions
            .iter()
            .map(|action| action["id"]["local"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "is-complete",
            "title",
            "title-history",
            "rename",
            "complete",
            "reject"
        ]
    );
    assert!(actions.iter().all(|action| {
        action["id"]["owner"]["kind"] == "entity" && action["id"]["owner"]["id"]["local"] == "task"
    }));
    assert!(
        actions
            .iter()
            .all(|action| action["id"]["local"] != "revision")
    );

    let model_without_task = domain_model! {
        contexts: [Planning],
        aggregates: [Project],
        entities: [Comment],
        identities: [CommentId],
        value_objects: [],
        services: [],
        commands: [],
        errors: [],
        query_groups: [],
    }
    .expect("domain model without task actions should be valid");
    assert!(model_without_task["actions"].as_array().unwrap().is_empty());
}
