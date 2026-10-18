//! Linux-specific VST3 module loading
//!
//! According to VST3 specification:
//! - ModuleEntry/ModuleExit functions are REQUIRED on Linux
//! - Must call ModuleEntry after dlopen and before GetPluginFactory
//! - Must call ModuleExit before dlclose or on program termination
//!
//! However, many VST3 plugins (especially older builds or third-party SDKs) only
//! export GetPluginFactory without ModuleEntry/ModuleExit. This loader treats
//! ModuleEntry/ModuleExit as optional: when they are missing the loader skips
//! calling them and goes straight to GetPluginFactory, which is the only
//! universally required entry point.

use super::{ModuleLoader, VstModule};
use crate::error::{Error, Result};
use std::path::Path;
use vst3::Steinberg::IPluginFactory;

#[cfg(target_os = "linux")]
use libloading::{Library, Symbol};

/// Function signature for ModuleEntry
type ModuleEntryFunc = unsafe extern "C" fn() -> bool;

/// Function signature for ModuleExit
type ModuleExitFunc = unsafe extern "C" fn() -> bool;

/// Function signature for GetPluginFactory
type GetPluginFactoryFunc = unsafe extern "C" fn() -> *mut IPluginFactory;

/// Linux VST3 module implementation
pub struct LinuxModule {
    /// libloading Library handle. Never read directly, but MUST outlive the symbols below:
    /// `get_factory_fn` is transmuted to `'static` and points into this library, so
    /// dropping it early would dangle it. Kept to own its lifetime.
    #[allow(dead_code)]
    library: Library,
    /// Path to the module
    path: std::path::PathBuf,
    /// ModuleExit function pointer (for cleanup) — None when plugin doesn't export it.
    module_exit: Option<Symbol<'static, ModuleExitFunc>>,
    /// GetPluginFactory function pointer
    get_factory_fn: Symbol<'static, GetPluginFactoryFunc>,
}

impl LinuxModule {
    /// Load a VST3 shared object using the correct Linux sequence.
    ///
    /// Tries the standard sequence: ModuleEntry → GetPluginFactory.
    /// Falls back to GetPluginFactory-only when ModuleEntry/ModuleExit are absent.
    fn load_internal(path: &Path) -> Result<Self> {
        unsafe {
            log::info!("=== Linux VST3 MODULE LOADING START ===");
            log::info!("Loading VST3 shared object: {}", path.display());

            // Step 1: Load the library
            log::debug!("Step 1: Loading shared object...");
            let library = Library::new(path).map_err(|e| {
                Error::PluginLoadFailed(format!("Failed to load shared object: {}", e))
            })?;
            log::debug!("Shared object loaded successfully");

            // Step 2 & 3: Try to get ModuleEntry/ModuleExit (optional).
            // Many VST3 plugins (especially those built with older SDKs or non-standard
            // toolchains) only export GetPluginFactory without ModuleEntry/ModuleExit.
            let module_exit: Option<Symbol<'static, ModuleExitFunc>> =
                match library.get::<ModuleEntryFunc>(b"ModuleEntry") {
                    Ok(entry_fn) => {
                        log::debug!("ModuleEntry function found — calling it...");
                        let entry_result = entry_fn();
                        if !entry_result {
                            log::warn!(
                                "ModuleEntry returned false — continuing anyway \
                                 (plugin may still work)"
                            );
                        } else {
                            log::debug!("ModuleEntry called successfully");
                        }

                        // ModuleExit should be present when ModuleEntry is
                        match library.get::<ModuleExitFunc>(b"ModuleExit") {
                            Ok(exit_fn) => {
                                log::debug!("ModuleExit function found");
                                let exit_static: Symbol<'static, ModuleExitFunc> =
                                    std::mem::transmute(exit_fn);
                                Some(exit_static)
                            }
                            Err(_) => {
                                log::warn!(
                                    "ModuleEntry present but ModuleExit missing — \
                                     skip exit cleanup"
                                );
                                None
                            }
                        }
                    }
                    Err(_) => {
                        log::info!(
                            "ModuleEntry not found — falling back to GetPluginFactory only"
                        );
                        None
                    }
                };

            // Step 4: Get GetPluginFactory function (REQUIRED — the only universal entry point)
            log::debug!("Step 4: Getting GetPluginFactory function...");
            let get_factory_fn = library
                .get::<GetPluginFactoryFunc>(b"GetPluginFactory")
                .map_err(|e| {
                    // Cleanup on failure if ModuleExit is available
                    if let Some(ref exit_fn) = module_exit {
                        let _ = exit_fn();
                    }
                    Error::PluginLoadFailed(format!("Failed to find GetPluginFactory: {}", e))
                })?;

            log::debug!("GetPluginFactory function found");

            // SAFETY: We extend the lifetime to 'static because we're storing these in the struct
            // and will ensure they're dropped before the library is unloaded
            let get_factory_fn: Symbol<'static, GetPluginFactoryFunc> =
                std::mem::transmute(get_factory_fn);

            log::info!("=== Linux VST3 MODULE LOADING COMPLETE ===");
            log::info!("Shared object loaded successfully: {}", path.display());

            Ok(LinuxModule {
                library,
                path: path.to_path_buf(),
                module_exit,
                get_factory_fn,
            })
        }
    }
}

impl VstModule for LinuxModule {
    fn get_factory(&self) -> Result<*mut IPluginFactory> {
        let factory = unsafe { (self.get_factory_fn)() };
        if factory.is_null() {
            Err(Error::PluginLoadFailed(
                "GetPluginFactory returned null".to_string(),
            ))
        } else {
            Ok(factory)
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for LinuxModule {
    fn drop(&mut self) {
        unsafe {
            log::debug!("=== Linux VST3 MODULE CLEANUP START ===");

            // Call ModuleExit only if it was available during loading
            if let Some(ref exit_fn) = self.module_exit {
                log::debug!("Calling ModuleExit...");
                let exit_result = exit_fn();
                if exit_result {
                    log::debug!("ModuleExit called successfully");
                } else {
                    log::warn!("ModuleExit returned false");
                }
            } else {
                log::debug!("No ModuleExit — skipping cleanup call");
            }

            // Library will be automatically unloaded when dropped
            log::debug!("Unloading shared object...");
            log::debug!("Shared object unloaded");

            log::debug!("=== Linux VST3 MODULE CLEANUP COMPLETE ===");
        }
    }
}

/// Linux module loader implementation
pub struct LinuxModuleLoader;

impl ModuleLoader for LinuxModuleLoader {
    fn load(path: &Path) -> Result<Box<dyn VstModule>> {
        let module = LinuxModule::load_internal(path)?;
        Ok(Box::new(module))
    }
}
