use std::marker::PhantomData;

use rostfrei_macros::{Command, Module};
use zs_core::{Aggregate, CommandHandler, DecisionContext};

struct Account<T: Send + Sync + 'static>(PhantomData<T>);

impl<T: Send + Sync + 'static> Aggregate for Account<T> {
    type Event = ();

    const AGGREGATE_TYPE: &'static str = "account";

    fn initial() -> Self {
        Self(PhantomData)
    }

    fn apply(&mut self, (): &Self::Event) {}
}

#[derive(Command)]
#[rostfrei(name = "account.open", version = 1, aggregate = Account<T>)]
struct OpenAccount<T: Send + Sync + 'static>(PhantomData<T>);

impl<T: Send + Sync + 'static> CommandHandler<OpenAccount<T>> for Account<T> {
    type Rejection = ();

    fn handle(
        _: &OpenAccount<T>,
        _: &mut DecisionContext<'_, Self>,
    ) -> Result<(), Self::Rejection> {
        Ok(())
    }
}

#[derive(Module)]
#[rostfrei(name = "accounts", commands(OpenAccount<T>))]
struct Accounts<T: Send + Sync + 'static>(PhantomData<T>);

fn main() {}
