use anyhow::{anyhow, Result};
use std::collections::HashMap;

use kclvm_ast::ast;
use kclvm_config::{
    modfile::get_vendor_home,
    settings::{SettingsFile, SettingsPathBuf},
};
use kclvm_error::{Diagnostic, Handler};
use kclvm_query::r#override::parse_override_spec;
use kclvm_runtime::{Context, PanicInfo, ValueRef};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;

const RESULT_SIZE: usize = 2048 * 2048;

#[allow(non_camel_case_types)]
pub type kclvm_char_t = i8;
#[allow(non_camel_case_types)]
pub type kclvm_size_t = i32;
#[allow(non_camel_case_types)]
pub type kclvm_context_t = std::ffi::c_void;
#[allow(non_camel_case_types)]
pub type kclvm_value_ref_t = std::ffi::c_void;

/// ExecProgramArgs denotes the configuration required to execute the KCL program.
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct ExecProgramArgs {
    pub work_dir: Option<String>,
    pub k_filename_list: Vec<String>,
    // -E key=value
    pub external_pkgs: Vec<ast::CmdExternalPkgSpec>,
    pub k_code_list: Vec<String>,
    // -D key=value
    pub args: Vec<ast::CmdArgSpec>,
    // -O override_spec
    pub overrides: Vec<ast::OverrideSpec>,
    // -S path_selector
    pub path_selector: Vec<String>,
    pub disable_yaml_result: bool,
    // Whether to apply overrides on the source code.
    pub print_override_ast: bool,
    // -r --strict-range-check
    pub strict_range_check: bool,
    // -n --disable-none
    pub disable_none: bool,
    // -v --verbose
    pub verbose: i32,
    // -d --debug
    pub debug: i32,
    // yaml/json: sort keys
    pub sort_keys: bool,
    /// Whether including schema type in JSON/YAML result
    pub include_schema_type_path: bool,
    // Whether to compile only.
    pub compile_only: bool,
    // Whether to compile diractroy recursively.
    pub recursive: bool,
    // plugin_agent is the address of plugin.
    #[serde(skip)]
    pub plugin_agent: u64,
}

impl ExecProgramArgs {
    /// [`get_package_maps_from_external_pkg`] gets the package name to package path mapping.
    pub fn get_package_maps_from_external_pkg(&self) -> HashMap<String, String> {
        let mut package_maps = HashMap::new();
        for external_pkg in &self.external_pkgs {
            package_maps.insert(external_pkg.pkg_name.clone(), external_pkg.pkg_path.clone());
        }
        package_maps
    }

    /// [`set_external_pkg_from_package_maps`] sets the package name to package path mapping.
    pub fn set_external_pkg_from_package_maps(&mut self, package_maps: HashMap<String, String>) {
        self.external_pkgs = package_maps
            .iter()
            .map(|(pkg_name, pkg_path)| ast::CmdExternalPkgSpec {
                pkg_name: pkg_name.clone(),
                pkg_path: pkg_path.clone(),
            })
            .collect();
    }
}

/// ExecProgramResult denotes the running result of the KCL program.
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct ExecProgramResult {
    pub json_result: String,
    pub yaml_result: String,
    pub log_message: String,
    pub err_message: String,
}

pub trait MapErrorResult {
    /// Map execute error message into the [`Result::Err`]
    fn map_err_to_result(self) -> Result<ExecProgramResult>
    where
        Self: Sized;
}

impl MapErrorResult for ExecProgramResult {
    /// Map execute error message into the [`Result::Err`]
    fn map_err_to_result(self) -> Result<ExecProgramResult>
    where
        Self: Sized,
    {
        if self.err_message.is_empty() {
            Ok(self)
        } else {
            Err(anyhow!(self.err_message))
        }
    }
}

impl MapErrorResult for Result<ExecProgramResult> {
    /// Map execute error message into the [`Result::Err`]
    fn map_err_to_result(self) -> Result<ExecProgramResult>
    where
        Self: Sized,
    {
        match self {
            Ok(result) => result.map_err_to_result(),
            Err(err) => Err(err),
        }
    }
}

impl ExecProgramArgs {
    /// Deserialize an instance of type [ExecProgramArgs] from a string of JSON text.
    pub fn from_str(s: &str) -> Self {
        if s.trim().is_empty() {
            return Default::default();
        }
        serde_json::from_str::<ExecProgramArgs>(s).expect(s)
    }

    /// Serialize the [ExecProgramArgs] structure as a String of JSON.
    pub fn to_json(&self) -> String {
        serde_json::ser::to_string(self).unwrap()
    }

    /// Get the input file list.
    pub fn get_files(&self) -> Vec<&str> {
        self.k_filename_list.iter().map(|s| s.as_str()).collect()
    }

    /// Get the [`kclvm_parser::LoadProgramOptions`] from the [`kclvm_runner::ExecProgramArgs`]
    pub fn get_load_program_options(&self) -> kclvm_parser::LoadProgramOptions {
        kclvm_parser::LoadProgramOptions {
            work_dir: self.work_dir.clone().unwrap_or_default(),
            vendor_dirs: vec![get_vendor_home()],
            package_maps: self.get_package_maps_from_external_pkg(),
            k_code_list: self.k_code_list.clone(),
            cmd_args: self.args.clone(),
            cmd_overrides: self.overrides.clone(),
            load_plugins: self.plugin_agent > 0,
            recursive: self.recursive,
            ..Default::default()
        }
    }
}

impl TryFrom<SettingsFile> for ExecProgramArgs {
    type Error = anyhow::Error;
    fn try_from(settings: SettingsFile) -> Result<Self, Self::Error> {
        let mut args = Self::default();
        if let Some(cli_configs) = settings.kcl_cli_configs {
            args.k_filename_list = cli_configs.files.unwrap_or_default();
            if args.k_filename_list.is_empty() {
                args.k_filename_list = cli_configs.file.unwrap_or_default();
            }
            args.strict_range_check = cli_configs.strict_range_check.unwrap_or_default();
            args.disable_none = cli_configs.disable_none.unwrap_or_default();
            args.verbose = cli_configs.verbose.unwrap_or_default() as i32;
            args.debug = cli_configs.debug.unwrap_or_default() as i32;
            args.sort_keys = cli_configs.sort_keys.unwrap_or_default();
            args.recursive = cli_configs.recursive.unwrap_or_default();
            args.include_schema_type_path =
                cli_configs.include_schema_type_path.unwrap_or_default();
            for override_str in &cli_configs.overrides.unwrap_or_default() {
                args.overrides.push(parse_override_spec(override_str)?);
            }
            args.path_selector = cli_configs.path_selector.unwrap_or_default();
            args.set_external_pkg_from_package_maps(
                cli_configs.package_maps.unwrap_or(HashMap::default()),
            )
        }
        if let Some(options) = settings.kcl_options {
            args.args = options
                .iter()
                .map(|o| ast::CmdArgSpec {
                    name: o.key.to_string(),
                    value: o.value.to_string(),
                })
                .collect();
        }
        Ok(args)
    }
}

impl TryFrom<SettingsPathBuf> for ExecProgramArgs {
    type Error = anyhow::Error;
    fn try_from(s: SettingsPathBuf) -> Result<Self, Self::Error> {
        let mut args: ExecProgramArgs = s.settings().clone().try_into()?;
        args.work_dir = s.path().clone().map(|p| p.to_string_lossy().to_string());
        Ok(args)
    }
}

/// A public struct named [Artifact] which wraps around the native library [libloading::Library].
pub struct Artifact(libloading::Library);

pub trait ProgramRunner {
    /// Run with the arguments [ExecProgramArgs] and return the program execute result that
    /// contains the planning result and the evaluation errors if any.
    fn run(&self, args: &ExecProgramArgs) -> Result<ExecProgramResult>;
}

impl ProgramRunner for Artifact {
    fn run(&self, args: &ExecProgramArgs) -> Result<ExecProgramResult> {
        unsafe {
            KclLibRunner::lib_kclvm_plugin_init(&self.0, args.plugin_agent)?;
            KclLibRunner::lib_kcl_run(&self.0, args)
        }
    }
}

impl Artifact {
    pub fn from_path<P: AsRef<OsStr>>(path: P) -> Result<Self> {
        let lib = unsafe { libloading::Library::new(path)? };
        Ok(Self(lib))
    }
}

#[derive(Debug, Default)]
pub struct KclLibRunnerOptions {
    pub plugin_agent_ptr: u64,
}

pub struct KclLibRunner {
    opts: KclLibRunnerOptions,
}

impl KclLibRunner {
    /// New a runner using the lib path and options.
    pub fn new(opts: Option<KclLibRunnerOptions>) -> Self {
        Self {
            opts: opts.unwrap_or_default(),
        }
    }

    /// Run kcl library with exec arguments.
    pub fn run(&self, lib_path: &str, args: &ExecProgramArgs) -> Result<ExecProgramResult> {
        unsafe {
            let lib = libloading::Library::new(std::path::PathBuf::from(lib_path).canonicalize()?)?;
            Self::lib_kclvm_plugin_init(&lib, self.opts.plugin_agent_ptr)?;
            Self::lib_kcl_run(&lib, args)
        }
    }
}

impl KclLibRunner {
    unsafe fn lib_kclvm_plugin_init(
        lib: &libloading::Library,
        plugin_method_ptr: u64,
    ) -> Result<()> {
        // get kclvm_plugin_init
        let kclvm_plugin_init: libloading::Symbol<
            unsafe extern "C" fn(
                fn_ptr: extern "C" fn(
                    method: *const i8,
                    args_json: *const i8,
                    kwargs_json: *const i8,
                ) -> *const i8,
            ),
        > = lib.get(b"kclvm_plugin_init")?;

        // get plugin_method
        let plugin_method_ptr = plugin_method_ptr;
        let plugin_method_ptr = (plugin_method_ptr as *const u64) as *const ()
            as *const extern "C" fn(
                method: *const i8,
                args: *const i8,
                kwargs: *const i8,
            ) -> *const i8;
        let plugin_method: extern "C" fn(
            method: *const i8,
            args: *const i8,
            kwargs: *const i8,
        ) -> *const i8 = std::mem::transmute(plugin_method_ptr);

        // register plugin agent
        kclvm_plugin_init(plugin_method);
        Ok(())
    }

    unsafe fn lib_kcl_run(
        lib: &libloading::Library,
        args: &ExecProgramArgs,
    ) -> Result<ExecProgramResult> {
        let kcl_run: libloading::Symbol<
            unsafe extern "C" fn(
                kclvm_main_ptr: u64, // main.k => kclvm_main
                option_len: kclvm_size_t,
                option_keys: *const *const kclvm_char_t,
                option_values: *const *const kclvm_char_t,
                strict_range_check: i32,
                disable_none: i32,
                disable_schema_check: i32,
                list_option_mode: i32,
                debug_mode: i32,
                result_buffer_len: *mut kclvm_size_t,
                result_buffer: *mut kclvm_char_t,
                warn_buffer_len: *mut kclvm_size_t,
                warn_buffer: *mut kclvm_char_t,
                log_buffer_len: *mut kclvm_size_t,
                log_buffer: *mut kclvm_char_t,
            ) -> kclvm_size_t,
        > = lib.get(b"_kcl_run")?;

        let kclvm_main: libloading::Symbol<u64> = lib.get(b"kclvm_main")?;
        let kclvm_main_ptr = kclvm_main.into_raw().into_raw() as u64;

        // CLI configs
        let option_len = args.args.len() as kclvm_size_t;

        let cstr_argv: Vec<_> = args
            .args
            .iter()
            .map(|arg| std::ffi::CString::new(arg.name.as_str()).unwrap())
            .collect();

        let mut p_argv: Vec<_> = cstr_argv
            .iter() // do NOT into_iter()
            .map(|arg| arg.as_ptr())
            .collect();
        p_argv.push(std::ptr::null());

        let p: *const *const kclvm_char_t = p_argv.as_ptr();
        let option_keys = p;

        let cstr_argv: Vec<_> = args
            .args
            .iter()
            .map(|arg| std::ffi::CString::new(arg.value.as_str()).unwrap())
            .collect();

        let mut p_argv: Vec<_> = cstr_argv
            .iter() // do NOT into_iter()
            .map(|arg| arg.as_ptr())
            .collect();
        p_argv.push(std::ptr::null());

        let p: *const *const kclvm_char_t = p_argv.as_ptr();
        let option_values = p;
        let strict_range_check = args.strict_range_check as i32;
        let disable_none = args.disable_none as i32;
        let disable_schema_check = 0; // todo
        let list_option_mode = 0; // todo
        let debug_mode = args.debug;

        // Exec json result
        let mut json_result = vec![0u8; RESULT_SIZE];
        let mut result_buffer_len = json_result.len() as i32 - 1;
        let json_result_buffer = json_result.as_mut_ptr() as *mut i8;

        // Exec warning data
        let mut warn_data = vec![0u8; RESULT_SIZE];
        let mut warn_buffer_len = warn_data.len() as i32 - 1;
        let warn_buffer = warn_data.as_mut_ptr() as *mut i8;

        // Exec log data
        let mut log_data = vec![0u8; RESULT_SIZE];
        let mut log_buffer_len = log_data.len() as i32 - 1;
        let log_buffer = log_data.as_mut_ptr() as *mut i8;

        let n = kcl_run(
            kclvm_main_ptr,
            option_len,
            option_keys,
            option_values,
            strict_range_check,
            disable_none,
            disable_schema_check,
            list_option_mode,
            debug_mode,
            &mut result_buffer_len,
            json_result_buffer,
            &mut warn_buffer_len,
            warn_buffer,
            &mut log_buffer_len,
            log_buffer,
        );
        let mut result = ExecProgramResult {
            log_message: String::from_utf8(log_data[0..log_buffer_len as usize].to_vec())?,
            ..Default::default()
        };
        if n > 0 {
            let s = std::str::from_utf8(&json_result[0..n as usize])?;
            match wrap_msg_in_result(s) {
                Ok(json) => result.json_result = json,
                Err(err) => result.err_message = err,
            }
        } else if n < 0 {
            let return_len = 0 - n;
            result.err_message = String::from_utf8(warn_data[0..return_len as usize].to_vec())?;
        }

        // Wrap runtime error into diagnostic style string.
        if !result.err_message.is_empty() {
            result.err_message = match Handler::default()
                .add_diagnostic(<PanicInfo as Into<Diagnostic>>::into(PanicInfo::from(
                    result.err_message.as_str(),
                )))
                .emit_to_string()
            {
                Ok(msg) => msg,
                Err(err) => err.to_string(),
            };
        }

        Ok(result)
    }
}

fn wrap_msg_in_result(msg: &str) -> Result<String, String> {
    let mut ctx = Context::new();
    // YAML is compatible with JSON. We can use YAML library for result parsing.
    let kcl_val = match ValueRef::from_yaml_stream(&mut ctx, msg) {
        Ok(msg) => msg,
        Err(err) => {
            return Err(err.to_string());
        }
    };
    if let Some(val) = kcl_val.get_by_key("__kcl_PanicInfo__") {
        if val.is_truthy() {
            return Err(msg.to_string());
        }
    }
    Ok(msg.to_string())
}
