#![cfg(feature = "async")]

use qubit_command::{CommandStderr, CommandStdin, CommandStdout};
use qubit_io::{AsyncClose, AsyncInput, AsyncOutput};

#[test]
fn public_stream_types_implement_qubit_io_traits() {
    fn input<T: AsyncInput<Item = u8>>() {}
    fn output<T: AsyncOutput<Item = u8> + AsyncClose>() {}
    input::<CommandStdout>();
    input::<CommandStderr>();
    output::<CommandStdin>();
}
