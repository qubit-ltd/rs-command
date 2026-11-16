use std::fs::File;
use std::io;
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use std::sync::Barrier;
use std::sync::Mutex;
#[cfg(test)]
use std::sync::OnceLock;

use qubit_operation::OperationScope;
use qubit_operation::ResourceRef;

use crate::execution::contracts::PreparedFiles;
use crate::execution::contracts::PreparedSink;
use crate::resource_run_options::FileSinkPolicy;

#[cfg(test)]
type RegistrationGate = Arc<(Barrier, Barrier)>;
#[cfg(test)]
type RegistrationGateSlot = OnceLock<Mutex<Option<RegistrationGate>>>;
#[cfg(test)]
static AFTER_FIRST_REGISTRATION: RegistrationGateSlot = OnceLock::new();

type RegistrationFailure = (io::Error, Option<PreparedFiles>, [Option<File>; 3]);

#[cfg(test)]
pub(super) fn gate_after_first_registration(gate: Option<RegistrationGate>) {
    *AFTER_FIRST_REGISTRATION
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("test registration gate lock should not poison") = gate;
}

pub(super) fn register_files(
    _scope: &OperationScope,
    reservations: &mut Vec<qubit_operation::ResourceReservation>,
    stdin: Option<File>,
    stdout: Option<(File, FileSinkPolicy)>,
    stderr: Option<(File, FileSinkPolicy)>,
) -> Result<PreparedFiles, Box<RegistrationFailure>> {
    let (stdout_file, stdout_policy) = stdout.map_or((None, None), |(file, policy)| (Some(file), Some(policy)));
    let (stderr_file, stderr_policy) = stderr.map_or((None, None), |(file, policy)| (Some(file), Some(policy)));
    let mut values = [stdin, stdout_file, stderr_file];
    let policies = [None, stdout_policy, stderr_policy];
    let mut refs: [Option<ResourceRef<Mutex<File>>>; 3] = [None, None, None];
    for index in 0..3 {
        let Some(file) = values[index].take() else {
            continue;
        };
        let Some(reservation) = reservations.pop() else {
            values[index] = Some(file);
            return Err(Box::new((
                io::Error::other("missing reserved resource slot"),
                partial(refs, policies),
                values,
            )));
        };
        match reservation.commit(Mutex::new(file)) {
            Ok(reference) => {
                refs[index] = Some(reference);
                #[cfg(test)]
                if refs.iter().filter(|reference| reference.is_some()).count() == 1 {
                    let gate = AFTER_FIRST_REGISTRATION
                        .get_or_init(|| Mutex::new(None))
                        .lock()
                        .expect("test registration gate lock should not poison")
                        .clone();
                    if let Some(gate) = gate {
                        gate.0.wait();
                        gate.1.wait();
                    }
                }
            }
            Err(error) => {
                let (_, recovered_file, _) = error.into_parts();
                values[index] = Some(recovered_file.into_inner().unwrap_or_else(|e| e.into_inner()));
                return Err(Box::new((
                    io::Error::other("resource registration failed"),
                    partial(refs, policies),
                    values,
                )));
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

fn build(mut refs: [Option<ResourceRef<Mutex<File>>>; 3], policies: [Option<FileSinkPolicy>; 3]) -> PreparedFiles {
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

pub(super) fn truncate_sinks(prepared: &PreparedFiles, scope: &OperationScope) -> io::Result<()> {
    for (name, sink) in [
        ("stdout", prepared.stdout.as_ref()),
        ("stderr", prepared.stderr.as_ref()),
    ] {
        let Some(sink) = sink else { continue };
        if sink.policy == FileSinkPolicy::Truncate {
            let mut file = sink.file.lock().map_err(|error| io::Error::other(error.to_string()))?;
            if let Err(error) = file.set_len(0) {
                record_effect(
                    scope,
                    sink.file.id(),
                    qubit_operation::EffectFact::Unknown,
                    format!("{name} sink truncation could not be confirmed"),
                );
                return Err(error);
            }
            record_effect(
                scope,
                sink.file.id(),
                qubit_operation::EffectFact::Known,
                format!("{name} sink truncated"),
            );
            use std::io::Seek;
            file.rewind()?;
        }
    }
    Ok(())
}

fn record_effect(
    scope: &OperationScope,
    resource_id: qubit_operation::ResourceId,
    fact: qubit_operation::EffectFact,
    description: String,
) {
    let record = qubit_operation::EffectRecord {
        operation_id: scope.id(),
        resource_id: Some(resource_id),
        fact,
        description: qubit_operation::DiagnosticSummary::new(&description),
    };
    if let Err(error) = scope.record_effect(record) {
        log::warn!("file side-effect evidence was not retained ({error:?}); file owner remains registered");
    }
}
