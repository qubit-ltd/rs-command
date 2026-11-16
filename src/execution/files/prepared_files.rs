use crate::execution::contracts::{PreparedFiles, PreparedSink};
use crate::resource_run_options::FileSinkPolicy;
use qubit_operation::{OperationScope, ResourceRef};
use std::fs::File;
use std::io;
use std::sync::Mutex;

pub(super) fn register_files(
    _scope: &OperationScope,
    reservations: &mut Vec<qubit_operation::ResourceReservation>,
    stdin: Option<File>,
    stdout: Option<(File, FileSinkPolicy)>,
    stderr: Option<(File, FileSinkPolicy)>,
) -> Result<PreparedFiles, (io::Error, Option<PreparedFiles>, [Option<File>; 3])> {
    let (stdout_file, stdout_policy) =
        stdout.map_or((None, None), |(file, policy)| (Some(file), Some(policy)));
    let (stderr_file, stderr_policy) =
        stderr.map_or((None, None), |(file, policy)| (Some(file), Some(policy)));
    let mut values = [stdin, stdout_file, stderr_file];
    let policies = [None, stdout_policy, stderr_policy];
    let mut refs: [Option<ResourceRef<Mutex<File>>>; 3] = [None, None, None];
    for index in 0..3 {
        let Some(file) = values[index].take() else {
            continue;
        };
        let Some(reservation) = reservations.pop() else {
            values[index] = Some(file);
            return Err((
                io::Error::other("missing reserved resource slot"),
                partial(refs, policies),
                values,
            ));
        };
        match reservation.commit(Mutex::new(file)) {
            Ok(reference) => refs[index] = Some(reference),
            Err(error) => {
                let (_, recovered_file, _) = error.into_parts();
                values[index] = Some(
                    recovered_file
                        .into_inner()
                        .unwrap_or_else(|e| e.into_inner()),
                );
                return Err((
                    io::Error::other("resource registration failed"),
                    partial(refs, policies),
                    values,
                ));
            }
        }
    }
    Ok(build(refs, policies))
}

fn partial(
    refs: [Option<ResourceRef<Mutex<File>>>; 3],
    policies: [Option<FileSinkPolicy>; 3],
) -> Option<PreparedFiles> {
    if refs.iter().all(Option::is_none) {
        None
    } else {
        Some(build(refs, policies))
    }
}

fn build(
    mut refs: [Option<ResourceRef<Mutex<File>>>; 3],
    policies: [Option<FileSinkPolicy>; 3],
) -> PreparedFiles {
    PreparedFiles::new(
        refs[0].take(),
        refs[1].take().map(|file| PreparedSink {
            file,
            policy: policies[1].expect("stdout policy accompanies stdout file"),
        }),
        refs[2].take().map(|file| PreparedSink {
            file,
            policy: policies[2].expect("stderr policy accompanies stderr file"),
        }),
    )
}

pub(super) fn truncate_sinks(prepared: &PreparedFiles) -> io::Result<()> {
    for sink in [prepared.stdout.as_ref(), prepared.stderr.as_ref()]
        .into_iter()
        .flatten()
    {
        if sink.policy == FileSinkPolicy::Truncate {
            let mut file = sink
                .file
                .lock()
                .map_err(|error| io::Error::other(error.to_string()))?;
            file.set_len(0)?;
            use std::io::Seek;
            file.rewind()?;
        }
    }
    Ok(())
}
