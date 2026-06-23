use std::{
    env, thread,
    time::{SystemTime, UNIX_EPOCH},
};

const TRACE_ENV: &str = "WASVY_CLI_TRACE";

pub(crate) fn log(message: impl AsRef<str>) {
    if env::var_os(TRACE_ENV).is_none() {
        return;
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();

    eprintln!(
        "[wasvy-cli trace {now}ms {:?}] {}",
        thread::current().id(),
        message.as_ref()
    );
}
