//! Polyglot persistence adapters (ADR-003 / impl §2.2): one module per store, each owning only
//! the data that belongs to it. Business rules (e.g. containment acyclicity) live in
//! `sysml-core` and are called from here — these adapters are I/O only.

pub mod neo4j;
pub mod objects;
pub mod postgres;

pub use neo4j::Neo4jStore;
pub use objects::ObjectStore;
pub use postgres::PostgresStore;
