//! Functional tests: parse a circuit and run it on the `qsim_statevec_cpu` simulator.

use std::path::Path;

use oq3_circuit::{parse_circuit, Gate};
use qsim_statevec_cpu::{
    execute_noiseless, QSimCircuit, QSimGate, SingleCtrlQubitOp, SingleQubitOp, TwoCtrlQubitOp,
};

// Map parsed gates to qsim_statevec_cpu's `QSimGate` representation.
fn to_qsim_gate(gate: &Gate) -> QSimGate {
    match (gate.name.as_str(), gate.qubits.as_slice()) {
        ("x", &[t]) => (SingleQubitOp::PauliX, t).into(),
        ("y", &[t]) => (SingleQubitOp::PauliY, t).into(),
        ("z", &[t]) => (SingleQubitOp::PauliZ, t).into(),
        ("h", &[t]) => (SingleQubitOp::Hadamard, t).into(),
        ("s", &[t]) => (SingleQubitOp::S, t).into(),
        ("t", &[t]) => (SingleQubitOp::T, t).into(),
        ("sx", &[t]) => (SingleQubitOp::SX, t).into(),
        ("cx", &[c, t]) => (SingleCtrlQubitOp::ControlledX, c, t).into(),
        ("cy", &[c, t]) => (SingleCtrlQubitOp::ControlledY, c, t).into(),
        ("cz", &[c, t]) => (SingleCtrlQubitOp::ControlledZ, c, t).into(),
        ("ccx", &[c0, c1, t]) => (TwoCtrlQubitOp::Toffoli, c0, c1, t).into(),
        _ => panic!("unsupported gate: {gate:?}"),
    }
}

fn simulate(file: &str) -> Vec<f64> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/circuits")
        .join(file);
    let source = std::fs::read_to_string(&path).expect("circuit file should be readable");
    let circuit = parse_circuit(&source).expect("circuit should parse");
    let qsim_circuit = QSimCircuit {
        num_qubits: circuit.num_qubits,
        gates: circuit.gates.iter().map(to_qsim_gate).collect(),
    };

    execute_noiseless(qsim_circuit)
        .expect("circuit should execute")
        .into()
}

#[test]
fn simple_circuit() {
    let measured = simulate("simple.qasm");
    let expected = [1.0, 1.0, 1.0, 0.5];

    assert_eq!(measured.len(), expected.len());
    for (i, (m, e)) in measured.iter().zip(expected).enumerate() {
        assert!(
            (m - e).abs() < 1e-9,
            "qubit {i}: measured {m}, expected {e}"
        );
    }
}
