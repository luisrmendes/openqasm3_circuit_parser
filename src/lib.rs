//! High-level circuit extraction from an OpenQASM 3 source file.
//!
//! This crate runs the full parse and semantic-analysis pipeline (via
//! [`oq3_semantics`]) and returns a simulator-agnostic [`ParsedCircuit`]
//! containing the qubit count and a flat list of [`GateApplication`] values.

use std::fs;
use std::path::Path;

use oq3_semantics::asg;
use oq3_semantics::semantic_error::SemanticErrorList;
use oq3_semantics::symbols::{SymbolIdResult, SymbolTable, SymbolType};
use oq3_semantics::syntax_to_semantics;
use oq3_semantics::types::Type;
use oq3_source_file::{ErrorTrait, SourceFile, SourceTrait};

/// A gate applied to a specific set of qubit indices.
#[derive(Debug, Clone)]
pub struct GateApplication {
    /// Gate name as it appears in the source (e.g. `"h"`, `"cx"`, `"ccx"`).
    pub name: String,
    /// Qubit indices (0-based) the gate is applied to, in argument order.
    pub qubits: Vec<u32>,
}

/// The result of parsing and extracting a circuit from an OpenQASM 3 file.
#[derive(Debug)]
pub struct ParsedCircuit {
    /// Total number of qubits declared (or inferred from usage if no declaration is present).
    pub num_qubits: u32,
    /// Ordered list of gate applications.
    pub gates: Vec<GateApplication>,
}

/// Parse an OpenQASM 3 source file and extract its circuit representation.
///
/// Returns an error string if the file cannot be read, contains parse/semantic
/// errors (one `path:line:col: message` line per error, including errors in
/// included files), uses unsupported features (parameterized gates, gate
/// modifiers, multi-dimensional qubit arrays), or references qubits outside
/// the declared range.
pub fn parse_circuit_file(file_path: &Path) -> Result<ParsedCircuit, String> {
    // `oq3_semantics` panics on unreadable files, so check up front.
    fs::read_to_string(file_path)
        .map_err(|e| format!("Unable to read {}: {e}", file_path.display()))?;

    let parse_result = syntax_to_semantics::parse_source_file(file_path, None::<&[&str]>);
    if parse_result.any_errors() {
        let mut errors = Vec::new();
        collect_syntax_errors(parse_result.syntax_result(), &mut errors);
        collect_semantic_errors(parse_result.take_context().errors(), &mut errors);
        return Err(errors.join("\n"));
    }

    let program = parse_result.program();
    let symbols = parse_result.symbol_table();

    let mut gates: Vec<GateApplication> = Vec::new();
    let mut declared_qubits: Option<u32> = None;

    for stmt in program.stmts() {
        match stmt {
            asg::Stmt::DeclareQuantum(declare) => {
                let sym_id = symbol_id_from_result(declare.name())?;
                let symbol = &symbols[&sym_id];
                match symbol.symbol_type() {
                    Type::Qubit => ensure_single_quantum_register(&mut declared_qubits, 1)?,
                    Type::QubitArray(dims) => {
                        let dims = dims.dims();
                        if dims.len() != 1 {
                            return Err(
                                "Only one-dimensional qubit arrays are supported".to_owned()
                            );
                        }
                        let size = u32::try_from(dims[0])
                            .map_err(|_| "Qubit array size does not fit in u32".to_owned())?;
                        ensure_single_quantum_register(&mut declared_qubits, size)?;
                    }
                    _ => {
                        return Err(
                            "Unexpected non-quantum declaration in quantum declaration".to_owned(),
                        )
                    }
                }
            }
            asg::Stmt::GateCall(gate_call) => {
                gates.push(extract_gate_application(gate_call, symbols)?);
            }
            asg::Stmt::AnnotatedStmt(stmt) => {
                if let asg::Stmt::GateCall(gate_call) = stmt.statement() {
                    gates.push(extract_gate_application(gate_call, symbols)?);
                } else {
                    return Err("Unsupported annotated statement".to_owned());
                }
            }
            // Gate definitions are not expanded: calls to a user-defined gate are
            // emitted by name, and the consumer decides how to map them.
            asg::Stmt::Include(_)
            | asg::Stmt::GateDefinition(_)
            | asg::Stmt::DeclareClassical(_)
            | asg::Stmt::InputDeclaration(_)
            | asg::Stmt::OutputDeclaration(_)
            | asg::Stmt::Pragma(_)
            | asg::Stmt::NullStmt => {}
            _ => return Err("Unsupported OpenQASM statement for circuit extraction".to_owned()),
        }
    }

    let inferred_num_qubits = gates
        .iter()
        .flat_map(|g| g.qubits.iter().copied())
        .max()
        .map(|x| x.saturating_add(1))
        .unwrap_or(0);

    let num_qubits = declared_qubits.unwrap_or(inferred_num_qubits);
    if num_qubits == 0 {
        return Err("No qubits were declared or used".to_owned());
    }
    if inferred_num_qubits > num_qubits {
        return Err("Operation references qubit outside declared range".to_owned());
    }

    Ok(ParsedCircuit { num_qubits, gates })
}

fn collect_syntax_errors(source_file: &SourceFile, out: &mut Vec<String>) {
    format_errors(source_file.syntax_ast().errors(), source_file.file_path(), out);
    for included in source_file.included() {
        collect_syntax_errors(included, out);
    }
}

fn collect_semantic_errors(errors: &SemanticErrorList, out: &mut Vec<String>) {
    format_errors(errors, errors.source_file_path(), out);
    for included in errors.include_errors() {
        collect_semantic_errors(included, out);
    }
}

fn format_errors<E: ErrorTrait>(errors: &[E], file_path: &Path, out: &mut Vec<String>) {
    if errors.is_empty() {
        return;
    }
    let source = fs::read_to_string(file_path).ok();
    for err in errors {
        let range = err.range();
        let (start, end) = (usize::from(range.start()), usize::from(range.end()));
        let mut message = match &source {
            Some(src) => {
                let (line, col) = line_col(src, start);
                format!("{}:{line}:{col}: {}", file_path.display(), err.message())
            }
            None => format!("{}@{start}: {}", file_path.display(), err.message()),
        };
        if let Some(snippet) = source.as_deref().and_then(|src| src.get(start..end)) {
            if !snippet.trim().is_empty() {
                message.push_str(&format!(" (near `{}`)", snippet.trim()));
            }
        }
        out.push(message);
    }
}

/// 1-based line and column of a byte offset in `source`.
fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let prefix = &source[..offset.min(source.len())];
    let line = prefix.matches('\n').count() + 1;
    let col = prefix.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, col)
}

fn extract_gate_application(
    gate_call: &asg::GateCall,
    symbols: &SymbolTable,
) -> Result<GateApplication, String> {
    if !gate_call.modifiers().is_empty() {
        return Err("Gate modifiers are not supported yet".to_owned());
    }
    if gate_call.params().is_some() {
        return Err("Parameterized gates are not supported yet".to_owned());
    }
    let name = symbol_name_from_result(gate_call.name(), symbols)?;
    let qubits = gate_call
        .qubits()
        .iter()
        .map(|expr| qubit_index_from_expr(expr, symbols))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(GateApplication { name, qubits })
}

fn ensure_single_quantum_register(current: &mut Option<u32>, new_size: u32) -> Result<(), String> {
    if let Some(existing) = current {
        if *existing != new_size {
            return Err("Multiple quantum register declarations are not supported".to_owned());
        }
        return Ok(());
    }
    *current = Some(new_size);
    Ok(())
}

fn symbol_id_from_result(
    symbol: &SymbolIdResult,
) -> Result<oq3_semantics::symbols::SymbolId, String> {
    symbol
        .clone()
        .map_err(|_| "Failed to resolve OpenQASM symbol".to_owned())
}

fn symbol_name_from_result(symbol: &SymbolIdResult, symbols: &SymbolTable) -> Result<String, String> {
    let symbol_id = symbol_id_from_result(symbol)?;
    Ok(symbols[&symbol_id].name().to_owned())
}

fn qubit_index_from_expr(expr: &asg::TExpr, symbols: &SymbolTable) -> Result<u32, String> {
    match expr.expression() {
        asg::Expr::GateOperand(gate_operand) => match gate_operand {
            asg::GateOperand::IndexedIdentifier(identifier) => {
                let index = identifier
                    .indexes()
                    .first()
                    .ok_or_else(|| "Missing qubit index".to_owned())?;
                match index {
                    asg::IndexOperator::ExpressionList(expressions) => {
                        let first_expr = expressions
                            .expressions
                            .first()
                            .ok_or_else(|| "Missing index expression".to_owned())?;
                        int_from_expr(first_expr)
                            .ok_or_else(|| "Qubit index must be a non-negative integer".to_owned())
                    }
                    asg::IndexOperator::SetExpression(_) => {
                        Err("Set-expression qubit indexing is not supported".to_owned())
                    }
                }
            }
            asg::GateOperand::HardwareQubit(hq) => {
                let digits: String = hq
                    .identifier()
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect();
                digits
                    .parse::<u32>()
                    .map_err(|_| "Failed to parse hardware qubit index".to_owned())
            }
            asg::GateOperand::Identifier(symbol) => {
                let symbol_id = symbol_id_from_result(symbol)?;
                let symbol = &symbols[&symbol_id];
                if symbol.symbol_type() == &Type::Qubit {
                    Ok(0)
                } else {
                    Err("Identifier gate operands require explicit indexing".to_owned())
                }
            }
        },
        _ => Err("Unsupported qubit operand expression".to_owned()),
    }
}

fn int_from_expr(expr: &asg::TExpr) -> Option<u32> {
    match expr.expression() {
        asg::Expr::Literal(asg::Literal::Int(int_lit)) => {
            if *int_lit.sign() {
                u32::try_from(*int_lit.value()).ok()
            } else {
                None
            }
        }
        asg::Expr::Cast(cast) => int_from_expr(cast.operand()),
        _ => None,
    }
}
