use std::path::{Path, PathBuf};

use oxc_allocator::Allocator;
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{TransformOptions, Transformer};

pub(crate) struct TranspiledModule {
    pub(crate) code: String,
    pub(crate) source_map: String,
}

pub(crate) fn typescript(
    source_path: &Path,
    module_name: &str,
    source: &str,
) -> Result<TranspiledModule, TranspileError> {
    let source_type =
        SourceType::from_path(source_path).map_err(|_| TranspileError::Extension {
            path: source_path.to_path_buf(),
        })?;
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if let Some(error) = parsed.diagnostics.first() {
        return Err(diagnostic(source_path, source, error));
    }

    let mut program = parsed.program;
    let semantic = SemanticBuilder::new()
        .with_excess_capacity(2.0)
        .build(&program);
    if let Some(error) = semantic.diagnostics.first() {
        return Err(diagnostic(source_path, source, error));
    }

    let transformed = Transformer::new(&allocator, source_path, &TransformOptions::default())
        .build_with_scoping(semantic.semantic.into_scoping(), &mut program);
    if let Some(error) = transformed.diagnostics.first() {
        return Err(diagnostic(source_path, source, error));
    }

    let generated = Codegen::new()
        .with_options(CodegenOptions {
            source_map_path: Some(PathBuf::from(module_name)),
            ..CodegenOptions::default()
        })
        .build(&program);
    let source_map = generated
        .map
        .ok_or_else(|| TranspileError::MissingSourceMap {
            path: source_path.to_path_buf(),
        })?
        .to_json_string();

    Ok(TranspiledModule {
        code: generated.code,
        source_map,
    })
}

fn diagnostic(path: &Path, source: &str, error: &OxcDiagnostic) -> TranspileError {
    TranspileError::Diagnostic {
        path: path.to_path_buf(),
        rendered: format!("{:?}", error.clone().with_source_code(source.to_owned())),
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum TranspileError {
    #[error("unsupported script extension: {path}", path = path.display())]
    Extension { path: PathBuf },
    #[error("TypeScript diagnostic in {path}:\n{rendered}", path = path.display())]
    Diagnostic { path: PathBuf, rendered: String },
    #[error("Oxc did not emit a source map for {path}", path = path.display())]
    MissingSourceMap { path: PathBuf },
}
