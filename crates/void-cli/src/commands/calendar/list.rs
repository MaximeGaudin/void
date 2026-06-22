use super::args::CalendarArgs;
use crate::service;
use crate::service::reads::{self, CalendarQuery};

pub(super) fn run_list(args: &CalendarArgs) -> anyhow::Result<()> {
    let db = crate::context::open_db()?;
    let query = CalendarQuery {
        day: args.day.as_deref(),
        from: args.from.as_deref(),
        to: args.to.as_deref(),
        connection: args.connection.as_deref(),
        connector: args.connector.as_deref(),
    };
    let value = reads::calendar_list(&db, &query)?;
    println!("{}", service::render(&value)?);
    Ok(())
}

pub(super) fn run_week() -> anyhow::Result<()> {
    let db = crate::context::open_db()?;
    let value = reads::calendar_week(&db)?;
    println!("{}", service::render(&value)?);
    Ok(())
}
