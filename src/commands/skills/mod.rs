//! Skills subcommand handlers.
//!
//! Bash: `cmd_skills()` case statement at bin/akm:403–446 dispatches subcommands.
//! Each submodule corresponds to one subcommand.

pub mod add;
pub mod clean;
pub mod edit;
pub mod import;
pub mod libgen;
pub mod list;
pub mod load;
pub mod loaded;
pub mod promote;
pub mod publish;
pub mod remove;
pub mod search;
pub mod session_setup;
pub mod status;
pub mod sync;
pub mod unload;
