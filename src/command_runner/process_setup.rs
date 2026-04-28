/*******************************************************************************
 *
 *    Copyright (c) 2026.
 *    Haixing Hu, Qubit Co. Ltd.
 *
 *    All rights reserved.
 *
 ******************************************************************************/
use std::{
    fs::File,
    path::Path,
    process::{
        Command as ProcessCommand,
        Stdio,
    },
};

use crate::command_stdin::CommandStdin;
use crate::{
    Command,
    CommandError,
    OutputStream,
};

/// Configures stdin for a process command.
pub(super) fn configure_stdin(
    command_text: &str,
    stdin: CommandStdin,
    process_command: &mut ProcessCommand,
) -> Result<Option<Vec<u8>>, CommandError> {
    match stdin {
        CommandStdin::Null => {
            process_command.stdin(Stdio::null());
            Ok(None)
        }
        CommandStdin::Inherit => {
            process_command.stdin(Stdio::inherit());
            Ok(None)
        }
        CommandStdin::Bytes(bytes) => {
            process_command.stdin(Stdio::piped());
            Ok(Some(bytes))
        }
        CommandStdin::File(path) => match File::open(&path) {
            Ok(file) => {
                process_command.stdin(Stdio::from(file));
                Ok(None)
            }
            Err(source) => Err(CommandError::OpenInputFailed {
                command: command_text.to_owned(),
                path,
                source,
            }),
        },
    }
}

/// Configures environment variables for a process command.
pub(super) fn configure_environment(command: &Command, process_command: &mut ProcessCommand) {
    if command.clears_environment() {
        process_command.env_clear();
    }
    for key in command.removed_environment() {
        process_command.env_remove(key);
    }
    for (key, value) in command.environment() {
        process_command.env(key, value);
    }
}

/// Opens an output tee file before spawning the child.
pub(super) fn open_output_file(
    command: &str,
    stream: OutputStream,
    path: Option<&Path>,
) -> Result<Option<File>, CommandError> {
    match path {
        Some(path) => {
            File::create(path)
                .map(Some)
                .map_err(|source| CommandError::OpenOutputFailed {
                    command: command.to_owned(),
                    stream,
                    path: path.to_path_buf(),
                    source,
                })
        }
        None => Ok(None),
    }
}
