//! One server: the engine it runs on, the machine it runs on, its changes,
//! and the factory once the engine is up (D27).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bastion_core::people::{Person, Role};
use bastion_protocol::{Change, Link};

use crate::api::factory::FactoryCell;
use crate::engine::Engine;
use crate::events::Bus;
use crate::factory::Factory;
use crate::machine::Machine;
use crate::people_store::PeopleStore;
use crate::stack::Stack;
use crate::store::SqlStore;

pub struct Server {
    pub engine: Arc<dyn Engine>,
    pub machine: Arc<dyn Machine>,
    pub events: Arc<Bus>,
    pub factory: FactoryCell,
    /// The stack this server runs on, and the checkout that holds it.
    pub stack: Stack,
    /// Where the server writes what it keeps outside the database, such as
    /// an audit export.
    pub data: PathBuf,
}

impl Server {
    /// A server on `engine`. Its link and its workers' comings and goings
    /// are published as they change.
    pub fn new(
        engine: Arc<dyn Engine>,
        machine: Arc<dyn Machine>,
        stack: Stack,
        data: PathBuf,
    ) -> Result<Arc<Self>, String> {
        let events = Arc::new(Bus::default());
        crate::api::engine::watch(events.clone(), engine.clone());
        let bus = events.clone();
        engine.on_functions_changed(Arc::new(move || bus.publish(Change::Console)))?;
        Ok(Arc::new(Server {
            engine,
            machine,
            events,
            factory: FactoryCell::default(),
            stack,
            data,
        }))
    }

    /// Once the engine is up: open the store, start the factory, and resume
    /// its jobs. `seed` is the first-run rules and grants (D26); `scratch` is
    /// where the factory keeps its bundles.
    /// `owner` is the person who started the server; they are its admin (D31).
    pub fn start_factory(self: &Arc<Self>, seed: String, scratch: PathBuf, owner: String) {
        let server = self.clone();
        crate::rt::spawn(async move {
            let mut told = false;
            let store = loop {
                if server.engine.link() == Link::Up
                    && let Some(store) = server.open_store(&seed, &mut told).await
                {
                    break store;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            };
            let admin = Person {
                name: owner,
                role: Role::Admin,
            };
            if let Err(e) = store.add_person(&admin).await {
                eprintln!("the owner was not added: {e}");
            }
            server.run(store, scratch);
        });
    }

    /// The store, or none while it cannot open yet. The stack starts its
    /// database a little after the engine; that wait is said once.
    async fn open_store(&self, seed: &str, told: &mut bool) -> Option<SqlStore> {
        match SqlStore::open(self.engine.clone(), seed).await {
            Ok(store) => Some(store),
            Err(e) if e.contains("function_not_found") => {
                if !std::mem::replace(told, true) {
                    eprintln!("the store waits for the database worker");
                }
                None
            }
            Err(e) => {
                eprintln!("the store did not open: {e}");
                None
            }
        }
    }

    fn run(&self, store: SqlStore, scratch: PathBuf) {
        let bus = self.events.clone();
        let on_change = Arc::new(move |id: &str| bus.publish(Change::Job { id: id.into() }));
        let factory = Factory::new(
            self.engine.clone(),
            Arc::new(store),
            self.machine.clone(),
            on_change,
            crate::factory::Dirs {
                scratch,
                data: Some(self.data.clone()),
            },
        );
        if let Err(e) = factory.start() {
            eprintln!("the factory did not start: {e}");
        }
        let _ = self.factory.set(factory);
        // Clients that read before the factory started read again.
        self.events.publish(Change::Job { id: String::new() });
    }
}
