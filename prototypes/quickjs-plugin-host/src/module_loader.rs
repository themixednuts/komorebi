use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use oxc_sourcemap::OwnedSourceMap;
use parking_lot::Mutex;
use rquickjs::{
    Ctx, Error, Module, Result,
    loader::{ImportAttributes, Loader, Resolver},
    module::Declared,
};

use crate::{path_key, transpile};

pub(crate) const HOST_MODULE: &str = "komorebi:host";
const HOST_SOURCE: &str = concat!(
    "export function focus(direction) {\n",
    "  return globalThis.__komorebi_focus(direction);\n",
    "}\n",
);

#[derive(Default)]
pub(crate) struct ModuleTelemetry {
    transformed: AtomicUsize,
    source_maps: Mutex<HashMap<String, String>>,
}

impl ModuleTelemetry {
    pub(crate) fn transformed_modules(&self) -> usize {
        self.transformed.load(Ordering::Relaxed)
    }

    pub(crate) fn remap_diagnostic(&self, diagnostic: &str) -> String {
        let source_maps = self.source_maps.lock();
        diagnostic
            .lines()
            .map(|line| remap_stack_line(line, &source_maps))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn remap_stack_line(line: &str, source_maps: &HashMap<String, String>) -> String {
    const PREFIX: &str = "komorebi-file:";
    let Some(start) = line.find(PREFIX) else {
        return line.to_owned();
    };
    let key_end = line[start + PREFIX.len()..]
        .find(|character: char| !character.is_ascii_hexdigit())
        .map_or(line.len(), |offset| start + PREFIX.len() + offset);
    let key = &line[start..key_end];
    let Some(path) = path_key::decode(key).ok() else {
        return line.to_owned();
    };
    let Some(position) = parse_position(&line[key_end..]) else {
        return line.replacen(key, &path_key::display(&path), 1);
    };
    let mapped = source_maps
        .get(key)
        .and_then(|json| OwnedSourceMap::from_json_string(json).ok())
        .and_then(|map| {
            let lookup = map.generate_lookup_table();
            map.lookup_token_approx(
                &lookup,
                u32::try_from(position.line.saturating_sub(1)).ok()?,
                u32::try_from(position.column.saturating_sub(1)).ok()?,
            )
        })
        .map_or((position.line, position.column), |token| {
            (
                usize::try_from(token.get_src_line()).unwrap_or(0) + 1,
                usize::try_from(token.get_src_col()).unwrap_or(0) + 1,
            )
        });
    format!(
        "{}{}:{}:{}{}",
        &line[..start],
        path_key::display(&path),
        mapped.0,
        mapped.1,
        &line[key_end + position.consumed..]
    )
}

struct StackPosition {
    line: usize,
    column: usize,
    consumed: usize,
}

fn parse_position(suffix: &str) -> Option<StackPosition> {
    let suffix = suffix.strip_prefix(':')?;
    let line_digits = suffix
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(suffix.len());
    let line = suffix.get(..line_digits)?.parse().ok()?;
    let column_suffix = suffix.get(line_digits..)?.strip_prefix(':')?;
    let column_digits = column_suffix
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(column_suffix.len());
    let column = column_suffix.get(..column_digits)?.parse().ok()?;
    Some(StackPosition {
        line,
        column,
        consumed: 1 + line_digits + 1 + column_digits,
    })
}

pub(crate) struct PluginResolver {
    root: PathBuf,
}

impl PluginResolver {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn resolve_file(&self, base: &str, specifier: &str) -> Result<PathBuf> {
        if !specifier.starts_with("./") && !specifier.starts_with("../") {
            return Err(Error::new_resolving_message(
                base,
                specifier,
                "bare package imports are disabled; only komorebi:host is available",
            ));
        }
        let base_path = path_key::decode(base)
            .map_err(|error| Error::new_resolving_message(base, specifier, error.to_string()))?;
        let Some(parent) = base_path.parent() else {
            return Err(Error::new_resolving_message(
                base,
                specifier,
                "importing module has no parent directory",
            ));
        };
        let candidate = parent.join(specifier.replace('/', "\\"));
        let resolved = candidates(&candidate)
            .find_map(|path| path.canonicalize().ok())
            .ok_or_else(|| {
                Error::new_resolving_message(base, specifier, "module file does not exist")
            })?;
        if !resolved.starts_with(&self.root) {
            return Err(Error::new_resolving_message(
                base,
                specifier,
                "module import escapes the configured plugin root",
            ));
        }
        Ok(resolved)
    }
}

impl Resolver for PluginResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> Result<String> {
        if name == HOST_MODULE {
            return Ok(HOST_MODULE.to_owned());
        }
        if path_key::is_encoded(name) {
            let path = path_key::decode(name)
                .map_err(|error| Error::new_resolving_message(base, name, error.to_string()))?;
            let canonical = path
                .canonicalize()
                .map_err(|error| Error::new_resolving_message(base, name, error.to_string()))?;
            if !canonical.starts_with(&self.root) {
                return Err(Error::new_resolving_message(
                    base,
                    name,
                    "module import escapes the configured plugin root",
                ));
            }
            return Ok(path_key::encode(&canonical));
        }
        self.resolve_file(base, name)
            .map(|path| path_key::encode(&path))
    }
}

pub(crate) struct PluginLoader {
    telemetry: Arc<ModuleTelemetry>,
    precompiled: HashMap<String, (String, String)>,
}

impl PluginLoader {
    pub(crate) fn new(telemetry: Arc<ModuleTelemetry>) -> Self {
        Self {
            telemetry,
            precompiled: HashMap::new(),
        }
    }

    pub(crate) fn with_precompiled(
        telemetry: Arc<ModuleTelemetry>,
        precompiled: HashMap<String, (String, String)>,
    ) -> Self {
        Self {
            telemetry,
            precompiled,
        }
    }

    fn load_file<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> Result<Module<'js, Declared>> {
        let path = path_key::decode(name)
            .map_err(|error| Error::new_loading_message(name, error.to_string()))?;
        if let Some((code, source_map)) = self.precompiled.remove(name) {
            self.telemetry.transformed.fetch_add(1, Ordering::Relaxed);
            self.telemetry
                .source_maps
                .lock()
                .insert(name.to_owned(), source_map);
            return Module::declare(ctx.clone(), name, code);
        }
        let source = fs::read_to_string(&path)
            .map_err(|error| Error::new_loading_message(name, error.to_string()))?;
        let extension = path.extension().and_then(|extension| extension.to_str());
        let code = match extension {
            Some("ts" | "mts") => {
                let transpiled = transpile::typescript(&path, name, &source)
                    .map_err(|error| Error::new_loading_message(name, error.to_string()))?;
                self.telemetry.transformed.fetch_add(1, Ordering::Relaxed);
                self.telemetry
                    .source_maps
                    .lock()
                    .insert(name.to_owned(), transpiled.source_map);
                transpiled.code
            }
            Some("js" | "mjs") => source,
            _ => {
                return Err(Error::new_loading_message(
                    name,
                    "only .ts, .mts, .js, and .mjs modules are supported",
                ));
            }
        };
        Module::declare(ctx.clone(), name, code)
    }
}

impl Loader for PluginLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> Result<Module<'js, Declared>> {
        if name == HOST_MODULE {
            Module::declare(ctx.clone(), name, HOST_SOURCE)
        } else {
            self.load_file(ctx, name)
        }
    }
}

fn candidates(path: &Path) -> impl Iterator<Item = PathBuf> {
    let mut candidates = Vec::with_capacity(5);
    if path.extension().is_some() {
        candidates.push(path.to_path_buf());
    } else {
        candidates.extend([path.with_extension("ts"), path.with_extension("mts")]);
        candidates.extend([path.with_extension("js"), path.with_extension("mjs")]);
        candidates.push(path.join("index.ts"));
    }
    candidates.into_iter()
}
