mod bootstrap;
mod command_registry;
mod commands;
mod desktop_window;
mod lifecycle;
mod state;

pub(crate) use bootstrap::run;
pub(crate) use state::AppState;

#[cfg(test)]
mod tests;
