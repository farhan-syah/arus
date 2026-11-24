use std::error::Error;
use std::io::{self, Write};
use std::process::ExitCode;

use arus_assets::Archive;
use arus_ingest::{Catalog, Collections, SyncOptions};
use arus_storage::RunStore;
use clap::Parser;
use serde::Serialize;
use time::OffsetDateTime;

use crate::arguments::{CatalogCommand, Cli, Command, DataCommand, RunsCommand};

type CommandResult<T> = Result<T, Box<dyn Error>>;

pub(crate) fn execute() -> ExitCode {
    match run(Cli::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> CommandResult<bool> {
    match cli.command {
        Command::Runs { archive, command } => {
            let opened = Archive::open(&archive)?;
            let store = RunStore::new(&opened);
            match command {
                RunsCommand::List => {
                    let index = store.load_index()?;
                    print_json(&index)?;
                }
                RunsCommand::Show { run_id } => {
                    let record = store.get_run(&run_id)?.ok_or_else(|| {
                        invalid_input(format!(
                            "Unknown run {run_id:?}. Run `runs list` to inspect archived runs."
                        ))
                    })?;
                    print_json(&record)?;
                }
            }
        }
        Command::Catalog { command } => {
            let catalog = Catalog::load(&cli.catalog)?;
            match command {
                CatalogCommand::List => print_json(&catalog.sources)?,
                CatalogCommand::Show { source_id } => {
                    let source = catalog
                        .sources
                        .iter()
                        .find(|source| source.id == source_id)
                        .ok_or_else(|| {
                            invalid_input(format!(
                                "Unknown source {source_id:?}. Run catalog list to select a source."
                            ))
                        })?;
                    print_json(source)?;
                }
                CatalogCommand::Check => {
                    let collections = Collections::load(&cli.collections)?;
                    arus_ingest::validate(&catalog, &collections)?;
                    print_json(&serde_json::json!({
                        "valid": true,
                        "sources": catalog.sources.len(),
                        "collections": collections.collections.len(),
                        "availability_checked": false,
                    }))?;
                }
            }
        }
        Command::Data { command } => {
            let catalog = Catalog::load(&cli.catalog)?;
            let mut collections = Collections::load(&cli.collections)?;
            arus_ingest::validate(&catalog, &collections)?;
            match command {
                DataCommand::Plan { usage } => {
                    declare_noncommercial(&mut collections, &[], usage.accept_noncommercial);
                    let archive = Archive::open(&cli.data_dir)?;
                    let items = arus_ingest::plan(
                        &catalog,
                        &collections,
                        &archive,
                        OffsetDateTime::now_utc(),
                    )?;
                    print_json(&items)?;
                }
                DataCommand::Status => {
                    let archive = Archive::open(&cli.data_dir)?;
                    let items = arus_ingest::status(
                        &catalog,
                        &collections,
                        &archive,
                        OffsetDateTime::now_utc(),
                    )?;
                    print_json(&items)?;
                }
                DataCommand::Sync {
                    collection_ids,
                    force,
                    usage,
                } => {
                    check_selection(&collections, &collection_ids)?;
                    declare_noncommercial(
                        &mut collections,
                        &collection_ids,
                        usage.accept_noncommercial,
                    );
                    let archive = Archive::open(&cli.data_dir)?;
                    let report = arus_ingest::sync(
                        &catalog,
                        &collections,
                        &archive,
                        &SyncOptions {
                            collection_ids,
                            force,
                        },
                    )?;
                    print_json(&report)?;
                    return Ok(report.failed == 0);
                }
                DataCommand::Refresh { usage } => {
                    declare_noncommercial(&mut collections, &[], usage.accept_noncommercial);
                    let archive = Archive::open(&cli.data_dir)?;
                    let report = arus_ingest::sync(
                        &catalog,
                        &collections,
                        &archive,
                        &SyncOptions {
                            collection_ids: Vec::new(),
                            force: false,
                        },
                    )?;
                    print_json(&report)?;
                    return Ok(report.failed == 0);
                }
            }
        }
    }
    Ok(true)
}

fn check_selection(collections: &Collections, selected: &[String]) -> CommandResult<()> {
    for id in selected {
        if !collections
            .collections
            .iter()
            .any(|collection| collection.id == *id)
        {
            return Err(invalid_input(format!(
                "Unknown collection {id:?}. Read the collection config and select an existing ID."
            ))
            .into());
        }
    }
    Ok(())
}

fn declare_noncommercial(collections: &mut Collections, selected: &[String], accepted: bool) {
    if !accepted {
        return;
    }
    for collection in &mut collections.collections {
        let applies = if selected.is_empty() {
            collection.enabled
        } else {
            selected.contains(&collection.id)
        };
        if applies {
            collection.accept_noncommercial = true;
        }
    }
}

fn print_json(value: &impl Serialize) -> CommandResult<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer_pretty(&mut output, value)?;
    writeln!(output)?;
    Ok(())
}

fn invalid_input(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use arus_ingest::Collections;
    use serde_json::json;

    use super::declare_noncommercial;

    fn collections() -> Result<Collections, Box<dyn Error>> {
        Ok(serde_json::from_value(json!({
            "schema_version": 1,
            "collections": [
                {
                    "id": "enabled",
                    "source_id": "source",
                    "description": "Enabled collection.",
                    "enabled": true,
                    "accept_noncommercial": false,
                    "parameters": {}
                },
                {
                    "id": "disabled",
                    "source_id": "source",
                    "description": "Disabled collection.",
                    "enabled": false,
                    "accept_noncommercial": false,
                    "parameters": {}
                }
            ]
        }))?)
    }

    #[test]
    fn usage_declaration_defaults_to_enabled_collections() -> Result<(), Box<dyn Error>> {
        let mut collections = collections()?;
        declare_noncommercial(&mut collections, &[], true);
        assert!(collections.collections[0].accept_noncommercial);
        assert!(!collections.collections[1].accept_noncommercial);
        Ok(())
    }

    #[test]
    fn explicit_usage_declaration_applies_only_to_selected_collection() -> Result<(), Box<dyn Error>>
    {
        let mut collections = collections()?;
        declare_noncommercial(&mut collections, &["disabled".to_owned()], true);
        assert!(!collections.collections[0].accept_noncommercial);
        assert!(collections.collections[1].accept_noncommercial);
        Ok(())
    }
}
